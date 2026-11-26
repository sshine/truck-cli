// These docs are spliced into README.md as its CLI section; the prose around it
// is hand-written in README.tpl at the repository root.

//! Clone and fetch every repository in a GitHub org.
//!
//! `truck sync <org>` lists the org with the authenticated GitHub CLI and clones
//! every repository whose directory is missing. `truck update <org>` runs
//! `git fetch --all --prune` in every clone that already exists. The org name
//! doubles as the directory below the base directory.
//!
//! ## Usage
//!
//! ```console
//! $ truck sync terranix
//! cloning terranix/terranix
//! truck: sync terranix: 1 cloned, 2 skipped, 0 failed in /Users/ssi/Projects/terranix
//!
//! $ truck sync --include-archived nix-tools
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
//! `~/Projects`, and `sync` creates `<DIR>/<org>` when it is missing. Archived
//! repositories are skipped unless `sync` is given `--include-archived`.
//!
//! Directories that already exist are never touched by `sync`, even when they
//! are not clones. `update` only looks at directories that are already there,
//! so archived repositories that were cloned explicitly keep working.
//!
//! ## Exit codes
//!
//! `0` when every repository succeeded, `1` when one or more clones or fetches
//! failed, and `2` when the arguments did not parse.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

/// How many repositories `gh repo list` may return in one call.
const REPO_LIMIT: usize = 5000;

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
        } => sync(&base, &org, include_archived),
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

fn sync(base: &Path, org: &str, include_archived: bool) -> Result<bool> {
    validate_org(org)?;
    let root = base.join(org);
    fs::create_dir_all(&root).with_context(|| format!("could not create {}", root.display()))?;

    let limit = REPO_LIMIT.to_string();
    let output = Command::new("gh")
        .args([
            "repo",
            "list",
            org,
            "--limit",
            &limit,
            "--json",
            "name,url,isArchived",
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
    if !include_archived {
        repos.retain(|repo| !repo.is_archived);
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));

    let (mut cloned, mut skipped, mut failed) = (0_usize, 0_usize, 0_usize);
    for repo in repos {
        let target = root.join(&repo.name);
        if target.exists() {
            skipped += 1;
            continue;
        }

        println!("cloning {org}/{}", repo.name);
        if clone_with_gh(org, &repo.name, &target) {
            cloned += 1;
            continue;
        }

        // gh may be missing, or its configured protocol may not work. The
        // listing already carries an HTTPS URL that plain git can use.
        eprintln!(
            "truck: gh could not clone {org}/{}; retrying with git",
            repo.name
        );
        let _ = fs::remove_dir_all(&target);
        if clone_with_git(&repo.url, &target) {
            cloned += 1;
        } else {
            failed += 1;
            eprintln!("truck: failed to clone {org}/{}", repo.name);
            // A half-written clone would be skipped forever; remove it so the
            // next sync retries instead.
            if let Err(error) = fs::remove_dir_all(&target) {
                eprintln!("truck: could not clean up {}: {error}", target.display());
            }
        }
    }

    println!(
        "truck: sync {org}: {cloned} cloned, {skipped} skipped, {failed} failed in {}",
        root.display()
    );
    Ok(failed == 0)
}

fn clone_with_gh(org: &str, name: &str, target: &Path) -> bool {
    let repository = format!("{org}/{name}");
    match Command::new("gh")
        .args(["repo", "clone"])
        .arg(&repository)
        .arg(target)
        .status()
    {
        Ok(status) => status.success(),
        Err(error) => {
            eprintln!("truck: could not run gh: {error}");
            false
        }
    }
}

fn clone_with_git(url: &str, target: &Path) -> bool {
    match Command::new("git")
        .args(["clone", "--", url])
        .arg(target)
        .status()
    {
        Ok(status) => status.success(),
        Err(error) => {
            eprintln!("truck: could not run git: {error}");
            false
        }
    }
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
