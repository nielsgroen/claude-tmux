//! Git worktree and branch management
//!
//! Provides operations for listing branches and managing worktrees.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use git2::Repository;

use super::GitContext;

impl GitContext {
    /// List all local branch names in the repository
    pub fn list_branches(repo_path: &Path) -> Result<Vec<String>> {
        let repo = Repository::discover(repo_path).context("Failed to open repository")?;
        let mut branches = Vec::new();

        for branch_result in repo.branches(Some(git2::BranchType::Local))? {
            let (branch, _) = branch_result?;
            if let Ok(Some(name)) = branch.name() {
                branches.push(name.to_string());
            }
        }

        // Sort with main/master first, then alphabetically
        branches.sort_by(|a, b| {
            let a_is_main = a == "main" || a == "master";
            let b_is_main = b == "main" || b == "master";
            match (a_is_main, b_is_main) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });

        Ok(branches)
    }

    /// Create a new worktree for a branch
    /// - If `is_new_branch` is true: creates a new branch from HEAD
    /// - If `is_new_branch` is false: uses an existing branch
    pub fn create_worktree(
        repo_path: &Path,
        worktree_path: &Path,
        branch_name: &str,
        is_new_branch: bool,
    ) -> Result<()> {
        let repo = Repository::discover(repo_path).context("Failed to open repository")?;

        // Sanitize branch name for worktree name (remove slashes)
        let worktree_name = branch_name.replace('/', "-");

        // Check if worktree path already exists
        if worktree_path.exists() {
            anyhow::bail!("Path '{}' already exists", worktree_path.display());
        }

        if is_new_branch {
            // Create new branch from HEAD, then create worktree
            let head = repo.head().context("Failed to get HEAD")?;
            let commit = head.peel_to_commit().context("Failed to get HEAD commit")?;

            // Create the branch first
            repo.branch(branch_name, &commit, false)
                .with_context(|| format!("Failed to create branch '{}'", branch_name))?;

            // Now create the worktree for this branch
            let refname = format!("refs/heads/{}", branch_name);
            let reference = repo
                .find_reference(&refname)
                .context("Failed to find created branch")?;

            repo.worktree(
                &worktree_name,
                worktree_path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&reference))),
            )
            .with_context(|| {
                format!(
                    "Failed to create worktree for new branch '{}' at '{}'",
                    branch_name,
                    worktree_path.display()
                )
            })?;
        } else {
            // Branch exists - create worktree for existing branch
            let refname = format!("refs/heads/{}", branch_name);
            let reference = repo
                .find_reference(&refname)
                .with_context(|| format!("Branch '{}' not found", branch_name))?;

            // Check if this branch is already checked out
            if let Ok(head) = repo.head() {
                if head.is_branch() {
                    if let Some(head_name) = head.shorthand() {
                        if head_name == branch_name {
                            anyhow::bail!(
                                "Branch '{}' is currently checked out in the main worktree. \
                                 Create a new branch instead, or checkout a different branch first.",
                                branch_name
                            );
                        }
                    }
                }
            }

            repo.worktree(
                &worktree_name,
                worktree_path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&reference))),
            )
            .with_context(|| {
                format!(
                    "Failed to create worktree for branch '{}' at '{}'. \
                     The branch may already be checked out in another worktree.",
                    branch_name,
                    worktree_path.display()
                )
            })?;
        }

        Ok(())
    }

    /// Delete the worktree at the given path using `git worktree remove`
    /// Returns an error if the worktree has uncommitted changes (unless force=true)
    pub fn delete_worktree(worktree_path: &Path, force: bool) -> Result<()> {
        // Verify it's actually a worktree
        let repo = Repository::discover(worktree_path).context("Failed to open repository")?;
        if !repo.is_worktree() {
            anyhow::bail!(
                "'{}' is not a worktree (it may be the main repository)",
                worktree_path.display()
            );
        }

        // Use git CLI for worktree removal - run from the worktree itself
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(worktree_path);
        cmd.arg("worktree").arg("remove");

        if force {
            cmd.arg("--force");
        }

        cmd.arg(worktree_path);

        let output = cmd
            .output()
            .context("Failed to execute git worktree remove")?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let hint = if stderr.contains("contains modified or untracked files") {
                " Commit or stash your changes first, or use force delete."
            } else if stderr.contains("is locked") {
                &format!(
                    " Unlock it first with: git worktree unlock {}",
                    worktree_path.display()
                )
            } else {
                ""
            };

            anyhow::bail!(
                "git worktree remove failed: {}.{}",
                stderr.trim(),
                hint
            )
        }
    }
}
