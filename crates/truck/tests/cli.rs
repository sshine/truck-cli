use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, ExitStatus};

use tempfile::TempDir;

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write script");
    let mut permissions = fs::metadata(path).expect("read metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make executable");
}

fn truck(args: &[&str], path: &Path) -> ExitStatus {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![path.to_path_buf()];
    paths.extend(std::env::split_paths(&inherited));
    let search_path = std::env::join_paths(paths).expect("join PATH");

    Command::new(env!("CARGO_BIN_EXE_truck"))
        .args(args)
        .env("PATH", search_path)
        .status()
        .expect("run truck")
}

fn write_gh(bin: &Path, repos: &Path, fail_clone: bool) {
    let clone = if fail_clone {
        "exit 1"
    } else {
        "mkdir -p \"$4\""
    };
    write_executable(
        &bin.join("gh"),
        &format!(
            r#"#!/bin/sh
case "$1 $2" in
  "repo list")
    cat '{repos}'
    ;;
  "repo clone")
    {clone}
    ;;
  *)
    echo "unexpected gh call: $*" >&2
    exit 1
    ;;
esac
"#,
            repos = repos.display()
        ),
    );
}

fn write_git(bin: &Path, log: &Path) {
    write_executable(
        &bin.join("git"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = clone ]; then mkdir -p \"$4\"; fi\n",
            log.display()
        ),
    );
}

#[test]
fn sync_clones_active_repositories_and_skips_the_rest() {
    let work = TempDir::new().expect("temp dir");
    let base = work.path().join("Projects");
    let bin = work.path().join("bin");
    let log = work.path().join("git.log");
    let repos = work.path().join("repos.json");
    fs::create_dir_all(&bin).expect("bin directory");

    fs::write(
        &repos,
        r#"[
  {"name": "archived", "url": "file:///archived.git", "isArchived": true},
  {"name": "active", "url": "file:///active.git", "isArchived": false},
  {"name": "present", "url": "file:///missing.git", "isArchived": false}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, false);
    write_git(&bin, &log);

    let present = base.join("terranix/present");
    fs::create_dir_all(&present).expect("pre-existing directory");
    fs::write(present.join("keep"), "sentinel").expect("sentinel");

    let root_arg = base.display().to_string();
    let status = truck(&["--base-dir", &root_arg, "sync", "terranix"], &bin);
    assert!(status.success(), "truck sync failed");

    assert!(base.join("terranix/active").is_dir());
    assert!(!base.join("terranix/archived").exists());
    assert_eq!(
        fs::read_to_string(present.join("keep")).expect("read sentinel"),
        "sentinel"
    );
}

#[test]
fn sync_include_archived_clones_archived_repositories() {
    let work = TempDir::new().expect("temp dir");
    let base = work.path().join("Projects");
    let bin = work.path().join("bin");
    let log = work.path().join("git.log");
    let repos = work.path().join("repos.json");
    fs::create_dir_all(&bin).expect("bin directory");

    fs::write(
        &repos,
        r#"[
  {"name": "archived", "url": "file:///archived.git", "isArchived": true}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, false);
    write_git(&bin, &log);

    let root_arg = base.display().to_string();
    let status = truck(
        &[
            "--base-dir",
            &root_arg,
            "sync",
            "--include-archived",
            "nix-tools",
        ],
        &bin,
    );
    assert!(status.success(), "truck sync failed");

    assert!(base.join("nix-tools/archived").is_dir());
}

#[test]
fn sync_falls_back_to_git_when_gh_clone_fails() {
    let work = TempDir::new().expect("temp dir");
    let base = work.path().join("Projects");
    let bin = work.path().join("bin");
    let log = work.path().join("git.log");
    let repos = work.path().join("repos.json");
    fs::create_dir_all(&bin).expect("bin directory");

    fs::write(
        &repos,
        r#"[
  {"name": "active", "url": "file:///active.git", "isArchived": false}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, true);
    write_git(&bin, &log);

    let root_arg = base.display().to_string();
    let status = truck(&["--base-dir", &root_arg, "sync", "terranix"], &bin);
    assert!(status.success(), "truck sync failed");

    assert!(base.join("terranix/active").is_dir());
    let log = fs::read_to_string(&log).expect("read git log");
    assert!(log.contains("clone -- file:///active.git"));
}

#[test]
fn update_fetches_only_existing_clones() {
    let work = TempDir::new().expect("temp dir");
    let base = work.path().join("Projects");
    let bin = work.path().join("bin");
    let log = work.path().join("git.log");
    fs::create_dir_all(&bin).expect("bin directory");

    write_executable(&bin.join("gh"), "#!/bin/sh\nexit 1\n");
    write_git(&bin, &log);

    for name in ["one", "not-a-clone", "two"] {
        fs::create_dir_all(base.join("terranix").join(name)).expect("repository directory");
    }
    fs::create_dir_all(base.join("terranix/one/.git")).expect("git directory");
    fs::create_dir_all(base.join("terranix/two/.git")).expect("git directory");

    let root_arg = base.display().to_string();
    let status = truck(&["--base-dir", &root_arg, "update", "terranix"], &bin);
    assert!(status.success(), "truck update failed");

    let log = fs::read_to_string(&log).expect("read git log");
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 2, "unexpected fetch calls: {lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/terranix/one fetch --all --prune"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("/terranix/two fetch --all --prune"))
    );
    assert!(!log.contains("not-a-clone"));
}
