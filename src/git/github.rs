//! GitHub CLI (gh) operations
//!
//! Provides pull request management through the GitHub CLI tool.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use git2::Repository;

/// Cached result of gh CLI availability check
static GH_AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Result of creating a pull request
#[derive(Debug)]
pub struct PullRequestResult {
    /// The URL of the created pull request
    pub url: String,
}

/// Information about an existing pull request
#[derive(Debug, Clone)]
pub struct PullRequestInfo {
    /// PR number
    pub number: u64,
    /// PR state (OPEN, CLOSED, MERGED)
    pub state: String,
    /// Whether the PR is mergeable (MERGEABLE, CONFLICTING, UNKNOWN)
    pub mergeable: String,
}

/// Check if the GitHub CLI (gh) is available and authenticated.
/// Result is cached for the lifetime of the program.
pub fn is_gh_available() -> bool {
    *GH_AVAILABLE.get_or_init(|| {
        // Check if gh is installed
        let version_check = Command::new("gh").arg("--version").output();

        if version_check.is_err() || !version_check.unwrap().status.success() {
            return false;
        }

        // Check if gh is authenticated
        let auth_check = Command::new("gh").args(["auth", "status"]).output();

        auth_check
            .map(|output| output.status.success())
            .unwrap_or(false)
    })
}

/// Check if the remote URL points to GitHub
pub fn is_github_remote(path: &Path) -> bool {
    get_remote_url(path)
        .map(|url| url.contains("github.com"))
        .unwrap_or(false)
}

/// Get the remote URL for the repository (first remote, usually "origin")
pub fn get_remote_url(path: &Path) -> Option<String> {
    let repo = Repository::discover(path).ok()?;
    let remotes = repo.remotes().ok()?;
    let remote_name = remotes.get(0)?;
    let remote = repo.find_remote(remote_name).ok()?;
    remote.url().map(|s| s.to_string())
}

/// Get the default branch name from the remote (usually "main" or "master")
pub fn get_default_branch(path: &Path) -> Option<String> {
    // Try to get from remote HEAD reference
    let repo = Repository::discover(path).ok()?;
    let remotes = repo.remotes().ok()?;
    let remote_name = remotes.get(0)?;

    // Try refs/remotes/origin/HEAD -> refs/remotes/origin/main
    let head_ref = format!("refs/remotes/{}/HEAD", remote_name);
    if let Ok(reference) = repo.find_reference(&head_ref) {
        if let Ok(resolved) = reference.resolve() {
            if let Some(name) = resolved.shorthand() {
                // Returns "origin/main" -> extract "main"
                return name.split('/').next_back().map(|s| s.to_string());
            }
        }
    }

    // Fallback: check if main or master exists
    let main_ref = format!("refs/remotes/{}/main", remote_name);
    if repo.find_reference(&main_ref).is_ok() {
        return Some("main".to_string());
    }

    let master_ref = format!("refs/remotes/{}/master", remote_name);
    if repo.find_reference(&master_ref).is_ok() {
        return Some("master".to_string());
    }

    // Ultimate fallback
    Some("main".to_string())
}

/// Create a pull request using the GitHub CLI
pub fn create_pull_request(
    path: &Path,
    title: &str,
    body: &str,
    base_branch: &str,
) -> Result<PullRequestResult> {
    if !is_gh_available() {
        anyhow::bail!("GitHub CLI (gh) is not available or not authenticated");
    }

    let mut cmd = Command::new("gh");
    cmd.current_dir(path);
    cmd.args(["pr", "create"]);
    cmd.args(["--title", title]);
    cmd.args(["--base", base_branch]);

    if !body.is_empty() {
        cmd.args(["--body", body]);
    } else {
        cmd.args(["--body", ""]);
    }

    let output = cmd.output().context("Failed to execute gh pr create")?;

    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(PullRequestResult { url })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr create failed: {}", stderr.trim())
    }
}

/// Get information about a PR for the current branch (if one exists)
pub fn get_pull_request_info(path: &Path) -> Option<PullRequestInfo> {
    if !is_gh_available() {
        return None;
    }

    let output = Command::new("gh")
        .current_dir(path)
        .args(["pr", "view", "--json", "number,url,state,mergeable"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);

    // Simple JSON parsing without adding a dependency
    // Format: {"number":123,"state":"OPEN","mergeable":"MERGEABLE"}
    let number = extract_json_u64(&json_str, "number")?;
    let state = extract_json_string(&json_str, "state")?;
    let mergeable =
        extract_json_string(&json_str, "mergeable").unwrap_or_else(|| "UNKNOWN".to_string());

    Some(PullRequestInfo {
        number,
        state,
        mergeable,
    })
}

/// Open the PR for the current branch in the browser
pub fn view_pull_request(path: &Path) -> Result<()> {
    if !is_gh_available() {
        anyhow::bail!("GitHub CLI (gh) is not available or not authenticated");
    }

    let output = Command::new("gh")
        .current_dir(path)
        .args(["pr", "view", "--web"])
        .output()
        .context("Failed to execute gh pr view")?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view failed: {}", stderr.trim())
    }
}

/// Merge the PR for the current branch
pub fn merge_pull_request(path: &Path, delete_branch: bool) -> Result<()> {
    if !is_gh_available() {
        anyhow::bail!("GitHub CLI (gh) is not available or not authenticated");
    }

    let mut cmd = Command::new("gh");
    cmd.current_dir(path);
    cmd.args(["pr", "merge", "--merge"]); // Use merge commit strategy

    if delete_branch {
        cmd.arg("--delete-branch");
    }

    let output = cmd.output().context("Failed to execute gh pr merge")?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr merge failed: {}", stderr.trim())
    }
}

/// Close the PR for the current branch without merging
pub fn close_pull_request(path: &Path) -> Result<()> {
    if !is_gh_available() {
        anyhow::bail!("GitHub CLI (gh) is not available or not authenticated");
    }

    let output = Command::new("gh")
        .current_dir(path)
        .args(["pr", "close"])
        .output()
        .context("Failed to execute gh pr close")?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr close failed: {}", stderr.trim())
    }
}

/// Simple helper to extract a string value from JSON
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":\"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Simple helper to extract a u64 value from JSON
fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}
