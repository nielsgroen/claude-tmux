//! Forge CLI (gh / glab) operations
//!
//! Provides pull/merge request management via the GitHub CLI (`gh`) or the
//! GitLab CLI (`glab`). A project can only support one forge; when multiple
//! remotes are configured, the first one listed by `git remote -v` whose URL
//! matches github.com or contains "gitlab" wins.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use git2::Repository;

/// Which forge (git hosting provider) a repository uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
}

impl Forge {
    /// CLI binary name for this forge.
    fn cli(self) -> &'static str {
        match self {
            Self::GitHub => "gh",
            Self::GitLab => "glab",
        }
    }

    /// The CLI subcommand noun ("pr" for gh, "mr" for glab).
    fn subcmd(self) -> &'static str {
        match self {
            Self::GitHub => "pr",
            Self::GitLab => "mr",
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
        }
    }
}

static GH_AVAILABLE: OnceLock<bool> = OnceLock::new();
static GLAB_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Result of creating a pull/merge request.
#[derive(Debug)]
pub struct PullRequestResult {
    /// URL of the created PR/MR.
    pub url: String,
}

/// Information about an existing pull/merge request.
#[derive(Debug, Clone)]
pub struct PullRequestInfo {
    /// PR/MR number.
    pub number: u64,
    /// Normalized state: OPEN / CLOSED / MERGED / LOCKED.
    pub state: String,
    /// Normalized mergeable status: MERGEABLE / CONFLICTING / UNKNOWN.
    pub mergeable: String,
}

/// Check whether the CLI for the given forge is installed and authenticated.
/// Cached for the lifetime of the program.
pub fn is_forge_cli_available(forge: Forge) -> bool {
    let cell = match forge {
        Forge::GitHub => &GH_AVAILABLE,
        Forge::GitLab => &GLAB_AVAILABLE,
    };
    *cell.get_or_init(|| {
        let cli = forge.cli();
        match Command::new(cli).arg("--version").output() {
            Ok(out) if out.status.success() => {}
            _ => return false,
        }
        Command::new(cli)
            .args(["auth", "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Detect which forge the repository's remotes point to, scanning remotes in
/// the order `git remote -v` reports and returning the first URL match.
pub fn detect_forge(path: &Path) -> Option<Forge> {
    let repo = Repository::discover(path).ok()?;
    let remotes = repo.remotes().ok()?;
    for i in 0..remotes.len() {
        let Some(name) = remotes.get(i) else { continue };
        let Ok(remote) = repo.find_remote(name) else { continue };
        let Some(url) = remote.url() else { continue };
        if url.contains("github.com") {
            return Some(Forge::GitHub);
        }
        if url.contains("gitlab") {
            return Some(Forge::GitLab);
        }
    }
    None
}

/// Detect the forge for a path, returning it only if its CLI is available.
pub fn available_forge(path: &Path) -> Option<Forge> {
    let forge = detect_forge(path)?;
    is_forge_cli_available(forge).then_some(forge)
}

/// Get the default branch name from the remote (usually "main" or "master").
pub fn get_default_branch(path: &Path) -> Option<String> {
    let repo = Repository::discover(path).ok()?;
    let remotes = repo.remotes().ok()?;
    let remote_name = remotes.get(0)?;

    let head_ref = format!("refs/remotes/{}/HEAD", remote_name);
    if let Ok(reference) = repo.find_reference(&head_ref) {
        if let Ok(resolved) = reference.resolve() {
            if let Some(name) = resolved.shorthand() {
                return name.split('/').next_back().map(|s| s.to_string());
            }
        }
    }

    let main_ref = format!("refs/remotes/{}/main", remote_name);
    if repo.find_reference(&main_ref).is_ok() {
        return Some("main".to_string());
    }

    let master_ref = format!("refs/remotes/{}/master", remote_name);
    if repo.find_reference(&master_ref).is_ok() {
        return Some("master".to_string());
    }

    Some("main".to_string())
}

fn require_forge(path: &Path) -> Result<Forge> {
    let forge = detect_forge(path)
        .ok_or_else(|| anyhow::anyhow!("No GitHub or GitLab remote detected"))?;
    if !is_forge_cli_available(forge) {
        anyhow::bail!(
            "{} CLI ({}) is not available or not authenticated",
            forge.display(),
            forge.cli()
        );
    }
    Ok(forge)
}

/// Create a pull/merge request using the detected forge CLI.
pub fn create_pull_request(
    path: &Path,
    title: &str,
    body: &str,
    base_branch: &str,
) -> Result<PullRequestResult> {
    let forge = require_forge(path)?;
    let mut cmd = Command::new(forge.cli());
    cmd.current_dir(path);

    match forge {
        Forge::GitHub => {
            cmd.args([
                "pr", "create", "--title", title, "--base", base_branch, "--body", body,
            ]);
        }
        Forge::GitLab => {
            cmd.args([
                "mr",
                "create",
                "--yes",
                "--title",
                title,
                "--description",
                body,
                "--target-branch",
                base_branch,
            ]);
        }
    }

    let output = cmd.output().with_context(|| {
        format!("Failed to execute {} {} create", forge.cli(), forge.subcmd())
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} {} create failed: {}",
            forge.cli(),
            forge.subcmd(),
            stderr.trim()
        );
    }

    // Both `gh` and `glab` print the created URL; glab emits a few lines, so
    // pick the last line that looks like a URL.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with("http"))
        .map(String::from)
        .unwrap_or_else(|| stdout.trim().to_string());

    Ok(PullRequestResult { url })
}

/// Get info about the PR/MR for the current branch, if one exists.
pub fn get_pull_request_info(path: &Path) -> Option<PullRequestInfo> {
    let forge = available_forge(path)?;

    let output = match forge {
        Forge::GitHub => Command::new("gh")
            .current_dir(path)
            .args(["pr", "view", "--json", "number,url,state,mergeable"])
            .output()
            .ok()?,
        Forge::GitLab => Command::new("glab")
            .current_dir(path)
            .args(["mr", "view", "-F", "json"])
            .output()
            .ok()?,
    };

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);

    match forge {
        Forge::GitHub => {
            let number = extract_json_u64(&json_str, "number")?;
            let state = extract_json_string(&json_str, "state")?;
            let mergeable = extract_json_string(&json_str, "mergeable")
                .unwrap_or_else(|| "UNKNOWN".to_string());
            Some(PullRequestInfo {
                number,
                state,
                mergeable,
            })
        }
        Forge::GitLab => {
            let number = extract_json_u64(&json_str, "iid")?;
            let raw_state = extract_json_string(&json_str, "state")?;
            let state = match raw_state.as_str() {
                "opened" => "OPEN",
                "closed" => "CLOSED",
                "merged" => "MERGED",
                "locked" => "LOCKED",
                other => other,
            }
            .to_string();
            let mergeable = match extract_json_string(&json_str, "merge_status").as_deref() {
                Some("can_be_merged") => "MERGEABLE",
                Some("cannot_be_merged") => "CONFLICTING",
                _ => "UNKNOWN",
            }
            .to_string();
            Some(PullRequestInfo {
                number,
                state,
                mergeable,
            })
        }
    }
}

/// Open the current branch's PR/MR in a browser.
pub fn view_pull_request(path: &Path) -> Result<()> {
    let forge = require_forge(path)?;
    let args: &[&str] = match forge {
        Forge::GitHub => &["pr", "view", "--web"],
        Forge::GitLab => &["mr", "view", "-w"],
    };

    let output = Command::new(forge.cli())
        .current_dir(path)
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute {} {} view", forge.cli(), forge.subcmd()))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} {} view failed: {}",
            forge.cli(),
            forge.subcmd(),
            stderr.trim()
        );
    }
}

/// Merge the current branch's PR/MR.
pub fn merge_pull_request(path: &Path, delete_branch: bool) -> Result<()> {
    let forge = require_forge(path)?;
    let mut cmd = Command::new(forge.cli());
    cmd.current_dir(path);

    match forge {
        Forge::GitHub => {
            cmd.args(["pr", "merge", "--merge"]);
            if delete_branch {
                cmd.arg("--delete-branch");
            }
        }
        Forge::GitLab => {
            cmd.args(["mr", "merge", "--yes"]);
            if delete_branch {
                cmd.arg("--remove-source-branch");
            }
        }
    }

    let output = cmd.output().with_context(|| {
        format!("Failed to execute {} {} merge", forge.cli(), forge.subcmd())
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} {} merge failed: {}",
            forge.cli(),
            forge.subcmd(),
            stderr.trim()
        );
    }
}

/// Close the current branch's PR/MR without merging.
pub fn close_pull_request(path: &Path) -> Result<()> {
    let forge = require_forge(path)?;
    let args: &[&str] = match forge {
        Forge::GitHub => &["pr", "close"],
        Forge::GitLab => &["mr", "close"],
    };

    let output = Command::new(forge.cli())
        .current_dir(path)
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute {} {} close", forge.cli(), forge.subcmd()))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} {} close failed: {}",
            forge.cli(),
            forge.subcmd(),
            stderr.trim()
        );
    }
}

/// Simple helper to extract a string value from JSON.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":\"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Simple helper to extract a u64 value from JSON.
fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}
