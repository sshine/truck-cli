// These docs are spliced into README.md as its CLI section; the prose around it
// is hand-written in README.tpl at the repository root.

//! Clone and fetch every repository in a GitHub org.
//!
//! `truck sync <org>` lists the org with the authenticated GitHub CLI, shows a
//! preflight summary and waits for approval before cloning every repository
//! whose directory is missing. `truck update <org>` runs
//! `git fetch --all --prune` in every clone that already exists. The org name
//! doubles as the directory below the base directory.
//!
//! ## Usage
//!
//! ```console
//! $ truck sync terranix
//! truck: terranix: 12 repositories, 4 archived (skipped)
//! truck: 8 new (approx. 1.2 MiB), 0 existing
//! Clone 8 repositories? [y/N] y
//! truck: logging clone output to /Users/ssi/.local/state/truck/logs/terranix-1764.log
//! [########################] 8/8 cloned terranix/terranix
//! truck: sync terranix: 8 cloned, 0 skipped, 0 failed in /Users/ssi/Projects/terranix
//!
//! $ truck sync --jobs 10 --include-archived --yes nix-tools
//! ...
//!
//! $ truck update terranix
//! fetching terranix/terranix
//! truck: update terranix: 3 fetched, 0 skipped, 0 failed in /Users/ssi/Projects/terranix
//!
//! $ truck --base-dir ~/src sync nix-tools
//! ```
//!
//! ## Options
//!
//! `--base-dir <DIR>` is the only configurable path. It defaults to
//! `~/Projects`, and `sync` creates `<DIR>/<org>` once cloning is approved.
//! Archived repositories are skipped unless `sync` is given
//! `--include-archived`. `--yes` skips the approval prompt for non-interactive
//! use, and `--jobs <N>` (default 5) sets how many clones run at once.
//!
//! Directories that already exist are never touched by `sync`, even when they
//! are not clones. `update` only looks at directories that are already there,
//! so archived repositories that were cloned explicitly keep working.
//!
//! ## Logs and progress
//!
//! Clone output no longer goes to the terminal. Each run appends it to
//! `$XDG_STATE_HOME/truck/logs/<org>-<timestamp>.log`, defaulting to
//! `~/.local/state/truck/logs/`, and the path is printed before cloning
//! starts. Each clone gets its own section, appended when it finishes, so
//! parallel clones do not interleave their output. The terminal shows an ANSI
//! progress line with the completed count, or plain status lines when stderr
//! is not a terminal.
//!
//! ## Exit codes
//!
//! `0` when every repository succeeded or there was nothing to do, `1` when a
//! clone or fetch failed or the preflight was declined, and `2` when the
//! arguments did not parse.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

/// How many repositories `gh repo list` may return in one call.
const REPO_LIMIT: usize = 5000;

/// Clear the current terminal line and return the cursor to its start.
const ANSI_CLEAR: &str = "\r\x1b[2K";

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Base directory below which each org is cloned (default: ~/Projects)
    #[arg(long, global = true, value_name = "DIR")]
    base_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Clone org repositories that are not present yet
    Sync {
        /// GitHub organization whose repositories to clone
        org: String,

        /// Also clone repositories that GitHub marks as archived
        #[arg(long)]
        include_archived: bool,

        /// Skip the preflight approval prompt
        #[arg(long, short = 'y')]
        yes: bool,

        /// Number of clones to run in parallel
        #[arg(long, short = 'j', value_name = "N", default_value_t = 5)]
        jobs: usize,
    },
    /// Run `git fetch --all --prune` in every existing clone
    Update {
        /// GitHub organization whose local clones to fetch
        org: String,
    },
}

#[derive(Debug, Deserialize)]
struct Repo {
    name: String,
    url: String,
    #[serde(rename = "isArchived")]
    is_archived: bool,
    /// Approximate repository size in kilobytes, as reported by GitHub.
    #[serde(rename = "diskUsage", default)]
    disk_usage: u64,
}

struct SyncState {
    done: usize,
    cloned: usize,
    failed: usize,
    log: File,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("truck: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    let base = match cli.base_dir {
        Some(dir) => dir,
        None => default_base_dir()?,
    };

    match cli.command {
        Action::Sync {
            org,
            include_archived,
            yes,
            jobs,
        } => sync(&base, &org, include_archived, yes, jobs),
        Action::Update { org } => update(&base, &org),
    }
}

fn default_base_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set; pass --base-dir")?;
    Ok(PathBuf::from(home).join("Projects"))
}

fn validate_org(org: &str) -> Result<()> {
    let mut chars = org.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        bail!("invalid org '{org}'");
    }
    Ok(())
}

fn sync(base: &Path, org: &str, include_archived: bool, yes: bool, jobs: usize) -> Result<bool> {
    validate_org(org)?;
    if jobs == 0 {
        bail!("--jobs must be at least 1");
    }
    let root = base.join(org);

    let limit = REPO_LIMIT.to_string();
    let output = Command::new("gh")
        .args([
            "repo",
            "list",
            org,
            "--limit",
            &limit,
            "--json",
            "name,url,isArchived,diskUsage",
        ])
        .output()
        .context("could not run gh; is the GitHub CLI installed?")?;
    if !output.status.success() {
        bail!(
            "could not list repositories in {org} (run `gh auth login`):\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let mut repos: Vec<Repo> =
        serde_json::from_slice(&output.stdout).context("gh returned unexpected JSON")?;
    let total = repos.len();
    let archived = repos.iter().filter(|repo| repo.is_archived).count();
    if !include_archived {
        repos.retain(|repo| !repo.is_archived);
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));

    let mut existing = 0_usize;
    let mut new_repos = Vec::new();
    for repo in repos {
        if root.join(&repo.name).exists() {
            existing += 1;
        } else {
            new_repos.push(repo);
        }
    }
    let approx_kb: u64 = new_repos.iter().map(|repo| repo.disk_usage).sum();

    if include_archived {
        println!("truck: {org}: {total} repositories ({archived} archived, included)");
    } else {
        println!("truck: {org}: {total} repositories, {archived} archived (skipped)");
    }
    println!(
        "truck: {} new (approx. {}), {existing} existing",
        new_repos.len(),
        human_size(approx_kb)
    );

    if new_repos.is_empty() {
        println!("truck: sync {org}: nothing to clone");
        return Ok(true);
    }

    if !yes && !approve(new_repos.len())? {
        eprintln!("truck: aborted (pass --yes for non-interactive use)");
        return Ok(false);
    }

    fs::create_dir_all(&root).with_context(|| format!("could not create {}", root.display()))?;
    let log_path = log_path(org)?;
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("could not open {}", log_path.display()))?;
    println!("truck: logging clone output to {}", log_path.display());
    writeln!(log, "# truck sync {org} at {}", timestamp())?;

    let total_new = new_repos.len();
    let workers = jobs.min(total_new).max(1);
    let next = AtomicUsize::new(0);
    let state = Mutex::new(SyncState {
        done: 0,
        cloned: 0,
        failed: 0,
        log,
    });

    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(repo) = new_repos.get(index) else {
                        break;
                    };
                    let target = root.join(&repo.name);

                    {
                        let state = state.lock().expect("sync state mutex");
                        progress(&state, total_new, "cloning", org, &repo.name);
                    }

                    let (success, section) = clone_repo(org, repo, &target);
                    {
                        let mut state = state.lock().expect("sync state mutex");
                        let _ = writeln!(state.log, "\n=== {org}/{} ===", repo.name);
                        let _ = state.log.write_all(section.as_bytes());
                        let _ = state.log.flush();
                        state.done += 1;
                        if success {
                            state.cloned += 1;
                        } else {
                            state.failed += 1;
                        }
                        let verb = if success { "cloned" } else { "failed" };
                        progress(&state, total_new, verb, org, &repo.name);
                    }

                    if !success {
                        eprintln!("truck: failed to clone {org}/{}", repo.name);
                    }
                }
            });
        }
    });

    let state = state.into_inner().expect("sync state mutex");
    if io::stderr().is_terminal() {
        eprint!("{ANSI_CLEAR}");
        let _ = io::stderr().flush();
    }

    println!(
        "truck: sync {org}: {} cloned, {existing} skipped, {} failed in {} (log: {})",
        state.cloned,
        state.failed,
        root.display(),
        log_path.display()
    );
    Ok(state.failed == 0)
}

/// Clone one repository, preferring gh and falling back to git. The returned
/// section is written to the shared log by the caller so parallel clones never
/// interleave.
fn clone_repo(org: &str, repo: &Repo, target: &Path) -> (bool, String) {
    let mut section = String::new();

    match gh_clone(org, &repo.name, target) {
        Ok(output) => {
            section.push_str(&format!("\n$ gh repo clone {org}/{}\n", repo.name));
            section.push_str(&String::from_utf8_lossy(&output.stdout));
            section.push_str(&String::from_utf8_lossy(&output.stderr));
            if output.status.success() {
                return (true, section);
            }
        }
        Err(error) => section.push_str(&format!("truck: could not run gh: {error}\n")),
    }

    // gh may be missing, or its configured protocol may not work. The listing
    // already carries an HTTPS URL that plain git can use.
    let _ = fs::remove_dir_all(target);
    section.push_str(&format!(
        "gh clone failed for {org}/{}; retrying with git\n",
        repo.name
    ));

    match git_clone(&repo.url, target) {
        Ok(output) => {
            section.push_str(&format!("\n$ git clone {}\n", repo.url));
            section.push_str(&String::from_utf8_lossy(&output.stdout));
            section.push_str(&String::from_utf8_lossy(&output.stderr));
            let success = output.status.success();
            if !success {
                // A half-written clone would be skipped forever; remove it so
                // the next sync retries instead.
                let _ = fs::remove_dir_all(target);
            }
            (success, section)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(target);
            section.push_str(&format!("truck: could not run git: {error}\n"));
            (false, section)
        }
    }
}

fn gh_clone(org: &str, name: &str, target: &Path) -> io::Result<Output> {
    Command::new("gh")
        .args(["repo", "clone"])
        .arg(format!("{org}/{name}"))
        .arg(target)
        .output()
}

fn git_clone(url: &str, target: &Path) -> io::Result<Output> {
    Command::new("git")
        .args(["clone", "--", url])
        .arg(target)
        .output()
}

fn approve(count: usize) -> Result<bool> {
    eprint!("Clone {count} repositories? [y/N] ");
    io::stderr().flush().ok();
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("could not read confirmation")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn progress(state: &SyncState, total: usize, verb: &str, org: &str, name: &str) {
    let width = 24;
    let filled = state
        .done
        .checked_mul(width)
        .and_then(|value| value.checked_div(total))
        .unwrap_or(width)
        .min(width);
    let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled));
    let done = state.done;

    if io::stderr().is_terminal() {
        eprint!("{ANSI_CLEAR}{bar} {done}/{total} {verb} {org}/{name}");
        let _ = io::stderr().flush();
    } else {
        eprintln!("{bar} {done}/{total} {verb} {org}/{name}");
    }
}

fn human_size(kilobytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = kilobytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn log_path(org: &str) -> Result<PathBuf> {
    let state_home = match env::var_os("XDG_STATE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => {
            let home = env::var_os("HOME")
                .context("HOME is not set; set XDG_STATE_HOME for the log directory")?;
            PathBuf::from(home).join(".local/state")
        }
    };
    let dir = state_home.join("truck").join("logs");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir.join(format!("{org}-{}.log", timestamp())))
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn update(base: &Path, org: &str) -> Result<bool> {
    validate_org(org)?;
    let root = base.join(org);
    if !root.is_dir() {
        bail!(
            "{} does not exist; run `truck sync {org}` first",
            root.display()
        );
    }

    let mut dirs = Vec::new();
    for entry in
        fs::read_dir(&root).with_context(|| format!("could not read {}", root.display()))?
    {
        let path = entry
            .with_context(|| format!("could not read an entry in {}", root.display()))?
            .path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();

    let (mut fetched, mut skipped, mut failed) = (0_usize, 0_usize, 0_usize);
    for dir in dirs {
        let name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !dir.join(".git").is_dir() {
            skipped += 1;
            continue;
        }

        println!("fetching {org}/{name}");
        let status = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["fetch", "--all", "--prune"])
            .status()
            .with_context(|| format!("could not run git for {org}/{name}"))?;

        if status.success() {
            fetched += 1;
        } else {
            failed += 1;
            eprintln!("truck: failed to fetch {org}/{name}");
        }
    }

    println!(
        "truck: update {org}: {fetched} fetched, {skipped} skipped, {failed} failed in {}",
        root.display()
    );
    Ok(failed == 0)
}
