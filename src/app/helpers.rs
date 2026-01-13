//! Helper utilities for the app module
//!
//! Pure functions for path manipulation and name sanitization.

use std::path::PathBuf;

/// Expand ~ to home directory in a path string
pub fn expand_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Sanitize a branch name for use as a session name
/// e.g., "feature/new-thing" -> "new-thing"
pub fn sanitize_for_session_name(branch: &str) -> String {
    branch
        .rsplit('/')
        .next()
        .unwrap_or(branch)
        .replace(['/', '\\', ' ', ':', '.'], "-")
}

/// Generate default worktree path from repo path and branch name
/// e.g., ~/repos/project + feature/foo -> ~/repos/project-foo
pub fn default_worktree_path(repo_path: &std::path::Path, branch: &str) -> PathBuf {
    let parent = repo_path.parent().unwrap_or(repo_path);
    let repo_name = repo_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");
    let branch_suffix = sanitize_for_session_name(branch);
    parent.join(format!("{}-{}", repo_name, branch_suffix))
}
