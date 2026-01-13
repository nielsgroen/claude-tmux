//! Git operations and GitHub CLI integration
//!
//! This module provides git functionality through libgit2 and GitHub CLI:
//! - `GitContext`: Detects and caches git state for a working directory
//! - `github`: GitHub CLI operations (PR management)
//! - `operations`: Core git operations (push, pull, fetch, commit, stage)
//! - `worktree`: Worktree and branch management

mod github;
mod operations;
mod worktree;

use std::path::{Path, PathBuf};

use git2::{Repository, StatusOptions};

// Re-export public API
pub use github::{
    close_pull_request, create_pull_request, get_default_branch, get_pull_request_info,
    is_gh_available, is_github_remote, merge_pull_request, view_pull_request, PullRequestInfo,
};

/// Git context for a session's working directory
#[derive(Debug, Clone)]
pub struct GitContext {
    /// Current branch name (or short commit hash if detached)
    pub branch: String,
    /// Whether there are staged changes ready to commit
    pub has_staged: bool,
    /// Whether there are unstaged changes in the working directory
    pub has_unstaged: bool,
    /// Whether this directory is a worktree (not the main checkout)
    pub is_worktree: bool,
    /// Path to the main repository (if this is a worktree)
    pub main_repo_path: Option<PathBuf>,
    /// Whether the branch has an upstream configured
    pub has_upstream: bool,
    /// Whether any remote is configured
    pub has_remote: bool,
    /// Commits ahead of upstream
    pub ahead: usize,
    /// Commits behind upstream
    pub behind: usize,
}

impl GitContext {
    /// Returns true if there are any uncommitted changes (staged or unstaged)
    pub fn is_dirty(&self) -> bool {
        self.has_staged || self.has_unstaged
    }

    /// Detect git context for a given path. Returns None if not a git repo.
    pub fn detect(path: &Path) -> Option<Self> {
        let repo = Repository::discover(path).ok()?;

        // Skip bare repositories
        if repo.is_bare() {
            return None;
        }

        // Get branch name
        let branch = match repo.head() {
            Ok(head) => {
                if head.is_branch() {
                    head.shorthand().unwrap_or("HEAD").to_string()
                } else {
                    // Detached HEAD - show short commit hash
                    head.peel_to_commit()
                        .map(|c| c.id().to_string()[..7].to_string())
                        .unwrap_or_else(|_| "HEAD".to_string())
                }
            }
            Err(_) => "HEAD".to_string(), // Empty repo or other edge case
        };

        // Check staged/unstaged state
        let mut status_opts = StatusOptions::new();
        status_opts
            .include_untracked(true)
            .include_ignored(false)
            .exclude_submodules(true);

        let (has_staged, has_unstaged) = repo
            .statuses(Some(&mut status_opts))
            .map(|statuses| {
                let mut staged = false;
                let mut unstaged = false;
                for entry in statuses.iter() {
                    let s = entry.status();
                    // Index (staged) changes
                    if s.intersects(
                        git2::Status::INDEX_NEW
                            | git2::Status::INDEX_MODIFIED
                            | git2::Status::INDEX_DELETED
                            | git2::Status::INDEX_RENAMED
                            | git2::Status::INDEX_TYPECHANGE,
                    ) {
                        staged = true;
                    }
                    // Worktree (unstaged) changes
                    if s.intersects(
                        git2::Status::WT_NEW
                            | git2::Status::WT_MODIFIED
                            | git2::Status::WT_DELETED
                            | git2::Status::WT_RENAMED
                            | git2::Status::WT_TYPECHANGE,
                    ) {
                        unstaged = true;
                    }
                }
                (staged, unstaged)
            })
            .unwrap_or((false, false));

        // Check if worktree
        let is_worktree = repo.is_worktree();
        let main_repo_path = if is_worktree {
            Some(repo.commondir().to_path_buf())
        } else {
            None
        };

        // Check if any remote is configured
        let has_remote = repo.remotes().map(|r| !r.is_empty()).unwrap_or(false);

        // Check if upstream is configured and get ahead/behind
        let (has_upstream, ahead, behind) = get_upstream_info(&repo);

        Some(GitContext {
            branch,
            has_staged,
            has_unstaged,
            is_worktree,
            main_repo_path,
            has_upstream,
            has_remote,
            ahead,
            behind,
        })
    }
}

/// Get upstream info: (has_upstream, ahead, behind)
fn get_upstream_info(repo: &Repository) -> (bool, usize, usize) {
    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return (false, 0, 0),
    };

    if !head.is_branch() {
        return (false, 0, 0); // Detached HEAD has no upstream
    }

    let branch_name = match head.shorthand() {
        Some(n) => n,
        None => return (false, 0, 0),
    };

    let local_branch = match repo.find_branch(branch_name, git2::BranchType::Local) {
        Ok(b) => b,
        Err(_) => return (false, 0, 0),
    };

    let upstream = match local_branch.upstream() {
        Ok(u) => u,
        Err(_) => return (false, 0, 0), // No upstream configured
    };

    // Has upstream, now get ahead/behind
    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return (true, 0, 0),
    };

    let upstream_oid = match upstream.get().target() {
        Some(oid) => oid,
        None => return (true, 0, 0),
    };

    match repo.graph_ahead_behind(local_oid, upstream_oid) {
        Ok((ahead, behind)) => (true, ahead, behind),
        Err(_) => (true, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_git_directory() {
        let dir = std::env::temp_dir();
        // temp_dir itself is unlikely to be a git repo
        // but we can't guarantee it, so just test the function doesn't panic
        let _ = GitContext::detect(&dir);
    }
}
