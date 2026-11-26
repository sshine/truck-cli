use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write script");
    let mut permissions = fs::metadata(path).expect("read metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make executable");
}

fn search_path(bin: &Path) -> std::ffi::OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(std::env::split_paths(&inherited));
    std::env::join_paths(paths).expect("join PATH")
}

fn truck(args: &[&str], bin: &Path, home: &Path, input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_truck"))
        .args(args)
        .env("PATH", search_path(bin))
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn truck");

    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(input.as_bytes()).expect("write stdin");
    }

    child.wait_with_output().expect("wait for truck")
}

fn write_gh(bin: &Path, repos: &Path, fail_clone: bool) {
    let clone = if fail_clone {
        "echo gh-clone-failed >&2; exit 1"
    } else {
        "echo \"gh-clone-output $3\"; mkdir -p \"$4\""
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
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = clone ]; then echo git-clone-output; mkdir -p \"$4\"; fi\n",
            log.display()
        ),
    );
}

fn first_log_entry(home: &Path) -> String {
    let dir = home.join("state/truck/logs");
    let entry = fs::read_dir(&dir)
        .expect("read log directory")
        .next()
        .expect("a log file")
        .expect("log entry");
    fs::read_to_string(entry.path()).expect("read log file")
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
  {"name": "archived", "url": "file:///archived.git", "isArchived": true, "diskUsage": 100},
  {"name": "active", "url": "file:///active.git", "isArchived": false, "diskUsage": 2048},
  {"name": "present", "url": "file:///missing.git", "isArchived": false, "diskUsage": 1}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, false);
    write_git(&bin, &log);

    let present = base.join("terranix/present");
    fs::create_dir_all(&present).expect("pre-existing directory");
    fs::write(present.join("keep"), "sentinel").expect("sentinel");

    let root_arg = base.display().to_string();
    let output = truck(
        &["--base-dir", &root_arg, "sync", "--yes", "terranix"],
        &bin,
        work.path(),
        "",
    );
    assert!(
        output.status.success(),
        "truck sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(base.join("terranix/active").is_dir());
    assert!(!base.join("terranix/archived").exists());
    assert_eq!(
        fs::read_to_string(present.join("keep")).expect("read sentinel"),
        "sentinel"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 archived (skipped)"), "stdout: {stdout}");
    assert!(
        stdout.contains("1 new (approx. 2.0 MiB)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("1 existing"), "stdout: {stdout}");
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
  {"name": "archived", "url": "file:///archived.git", "isArchived": true, "diskUsage": 10}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, false);
    write_git(&bin, &log);

    let root_arg = base.display().to_string();
    let output = truck(
        &[
            "--base-dir",
            &root_arg,
            "sync",
            "--include-archived",
            "--yes",
            "nix-tools",
        ],
        &bin,
        work.path(),
        "",
    );
    assert!(
        output.status.success(),
        "truck sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(base.join("nix-tools/archived").is_dir());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 archived, included"), "stdout: {stdout}");
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
  {"name": "active", "url": "file:///active.git", "isArchived": false, "diskUsage": 10}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, true);
    write_git(&bin, &log);

    let root_arg = base.display().to_string();
    let output = truck(
        &["--base-dir", &root_arg, "sync", "--yes", "terranix"],
        &bin,
        work.path(),
        "",
    );
    assert!(
        output.status.success(),
        "truck sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(base.join("terranix/active").is_dir());
    assert!(first_log_entry(work.path()).contains("git-clone-output"));
}

#[test]
fn sync_asks_for_approval_and_logs_clone_output() {
    let work = TempDir::new().expect("temp dir");
    let base = work.path().join("Projects");
    let bin = work.path().join("bin");
    let log = work.path().join("git.log");
    let repos = work.path().join("repos.json");
    fs::create_dir_all(&bin).expect("bin directory");

    fs::write(
        &repos,
        r#"[
  {"name": "active", "url": "file:///active.git", "isArchived": false, "diskUsage": 10}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, false);
    write_git(&bin, &log);

    let root_arg = base.display().to_string();
    let declined = truck(
        &["--base-dir", &root_arg, "sync", "terranix"],
        &bin,
        work.path(),
        "n\n",
    );
    assert!(!declined.status.success());
    assert!(!base.join("terranix/active").exists());
    let stderr = String::from_utf8_lossy(&declined.stderr);
    assert!(stderr.contains("Clone 1 repositories?"), "stderr: {stderr}");
    assert!(stderr.contains("aborted"), "stderr: {stderr}");

    let approved = truck(
        &["--base-dir", &root_arg, "sync", "terranix"],
        &bin,
        work.path(),
        "y\n",
    );
    assert!(
        approved.status.success(),
        "truck sync failed: {}",
        String::from_utf8_lossy(&approved.stderr)
    );
    assert!(base.join("terranix/active").is_dir());

    let stdout = String::from_utf8_lossy(&approved.stdout);
    assert!(
        stdout.contains("logging clone output to"),
        "stdout: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&approved.stderr);
    assert!(
        !stderr.contains("gh-clone-output"),
        "clone output leaked to stderr: {stderr}"
    );
    assert!(first_log_entry(work.path()).contains("gh-clone-output"));
}

#[test]
fn sync_clones_multiple_repositories_in_parallel() {
    let work = TempDir::new().expect("temp dir");
    let base = work.path().join("Projects");
    let bin = work.path().join("bin");
    let log = work.path().join("git.log");
    let repos = work.path().join("repos.json");
    fs::create_dir_all(&bin).expect("bin directory");

    fs::write(
        &repos,
        r#"[
  {"name": "one", "url": "file:///one.git", "isArchived": false, "diskUsage": 1},
  {"name": "two", "url": "file:///two.git", "isArchived": false, "diskUsage": 1},
  {"name": "three", "url": "file:///three.git", "isArchived": false, "diskUsage": 1},
  {"name": "four", "url": "file:///four.git", "isArchived": false, "diskUsage": 1}
]"#,
    )
    .expect("write listing");

    write_gh(&bin, &repos, false);
    write_git(&bin, &log);

    let root_arg = base.display().to_string();
    let output = truck(
        &[
            "--base-dir",
            &root_arg,
            "sync",
            "--yes",
            "--jobs",
            "2",
            "terranix",
        ],
        &bin,
        work.path(),
        "",
    );
    assert!(
        output.status.success(),
        "truck sync failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let log = first_log_entry(work.path());
    for name in ["one", "two", "three", "four"] {
        assert!(base.join("terranix").join(name).is_dir(), "missing {name}");
        assert!(
            log.contains(&format!("=== terranix/{name} ===")),
            "missing log section for {name}:\n{log}"
        );
        assert!(
            log.contains(&format!("gh-clone-output terranix/{name}")),
            "missing clone output for {name}:\n{log}"
        );
    }
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
    let output = truck(
        &["--base-dir", &root_arg, "update", "terranix"],
        &bin,
        work.path(),
        "",
    );
    assert!(
        output.status.success(),
        "truck update failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

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
