use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{
    AutotagOption, Cred, CredentialType, FetchOptions, PushOptions, RemoteCallbacks, Repository,
    StatusOptions,
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
}

impl GitContext {
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
        let (has_upstream, ahead, behind) = Self::get_upstream_info(&repo);

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

    /// Stage all changes (like git add -A)
    pub fn stage_all(path: &Path) -> Result<()> {
        let repo = Repository::discover(path).context("Failed to open repository")?;

        let mut index = repo.index().context("Failed to get index")?;

        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .context("Failed to stage files")?;

        // Also remove deleted files from index
        index
            .update_all(["*"].iter(), None)
            .context("Failed to update index")?;

        index.write().context("Failed to write index")?;

        Ok(())
    }

    /// Commit staged changes with a message
    pub fn commit(path: &Path, message: &str) -> Result<()> {
        let repo = Repository::discover(path).context("Failed to open repository")?;

        let mut index = repo.index().context("Failed to get index")?;
        let tree_oid = index.write_tree().context("Failed to write tree")?;
        let tree = repo.find_tree(tree_oid).context("Failed to find tree")?;

        let signature = repo.signature().context("Failed to get signature")?;

        let parent_commit = match repo.head() {
            Ok(head) => Some(head.peel_to_commit().context("Failed to get HEAD commit")?),
            Err(_) => None, // Initial commit
        };

        let parents: Vec<&git2::Commit> = parent_commit.iter().collect();

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
        .context("Failed to create commit")?;

        Ok(())
    }

    /// Push and set upstream (like git push -u origin branch)
    pub fn push_set_upstream(path: &Path) -> Result<()> {
        let repo = Repository::discover(path).context("Failed to open repository")?;

        let head = repo.head().context("Failed to get HEAD")?;
        if !head.is_branch() {
            anyhow::bail!("Cannot push: HEAD is detached");
        }

        let branch_name = head
            .shorthand()
            .ok_or_else(|| anyhow::anyhow!("Invalid branch name"))?
            .to_string();

        // Find the first remote (usually "origin")
        let remotes = repo.remotes().context("Failed to list remotes")?;
        let remote_name = remotes
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("No remotes configured"))?;

        let mut remote = repo
            .find_remote(remote_name)
            .context("Failed to find remote")?;

        let callbacks = Self::create_callbacks();
        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(callbacks);

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);

        remote
            .push(&[&refspec], Some(&mut push_options))
            .context("Push failed")?;

        // Set upstream tracking branch
        let mut local_branch = repo
            .find_branch(&branch_name, git2::BranchType::Local)
            .context("Failed to find local branch")?;

        let upstream_name = format!("{}/{}", remote_name, branch_name);
        local_branch
            .set_upstream(Some(&upstream_name))
            .context("Failed to set upstream")?;

        Ok(())
    }

    /// Push to the upstream remote using libgit2
    pub fn push(path: &Path) -> Result<()> {
        let repo = Repository::discover(path).context("Failed to open repository")?;

        let head = repo.head().context("Failed to get HEAD")?;
        if !head.is_branch() {
            anyhow::bail!("Cannot push: HEAD is detached");
        }

        let branch_name = head
            .shorthand()
            .ok_or_else(|| anyhow::anyhow!("Invalid branch name"))?;

        let local_branch = repo
            .find_branch(branch_name, git2::BranchType::Local)
            .context("Failed to find local branch")?;

        let upstream = local_branch
            .upstream()
            .context("No upstream branch configured")?;

        // Get remote name from upstream ref (e.g., "origin/main" -> "origin")
        let upstream_name = upstream
            .name()
            .context("Invalid upstream name")?
            .ok_or_else(|| anyhow::anyhow!("Upstream name is not valid UTF-8"))?;

        let remote_name = upstream_name
            .split('/')
            .next()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine remote name"))?;

        let mut remote = repo
            .find_remote(remote_name)
            .context("Failed to find remote")?;

        let callbacks = Self::create_callbacks();
        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(callbacks);

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);

        remote
            .push(&[&refspec], Some(&mut push_options))
            .context("Push failed")?;

        Ok(())
    }

    /// Pull (fetch + fast-forward merge) from upstream using libgit2
    pub fn pull(path: &Path) -> Result<()> {
        let repo = Repository::discover(path).context("Failed to open repository")?;

        let head = repo.head().context("Failed to get HEAD")?;
        if !head.is_branch() {
            anyhow::bail!("Cannot pull: HEAD is detached");
        }

        let branch_name = head
            .shorthand()
            .ok_or_else(|| anyhow::anyhow!("Invalid branch name"))?;

        let local_branch = repo
            .find_branch(branch_name, git2::BranchType::Local)
            .context("Failed to find local branch")?;

        let upstream = local_branch
            .upstream()
            .context("No upstream branch configured")?;

        // Get remote name from upstream ref
        let upstream_name = upstream
            .name()
            .context("Invalid upstream name")?
            .ok_or_else(|| anyhow::anyhow!("Upstream name is not valid UTF-8"))?;

        let remote_name = upstream_name
            .split('/')
            .next()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine remote name"))?;

        let mut remote = repo
            .find_remote(remote_name)
            .context("Failed to find remote")?;

        // Fetch
        let callbacks = Self::create_callbacks();
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        fetch_options.download_tags(AutotagOption::Auto);

        remote
            .fetch(&[branch_name], Some(&mut fetch_options), None)
            .context("Fetch failed")?;

        // Get the fetch head
        let fetch_head = repo
            .find_reference("FETCH_HEAD")
            .context("Failed to find FETCH_HEAD")?;

        let fetch_commit = repo
            .reference_to_annotated_commit(&fetch_head)
            .context("Failed to get fetch commit")?;

        // Perform fast-forward merge
        let (analysis, _) = repo
            .merge_analysis(&[&fetch_commit])
            .context("Merge analysis failed")?;

        if analysis.is_up_to_date() {
            // Already up to date
            return Ok(());
        }

        if analysis.is_fast_forward() {
            // Fast-forward
            let target_oid = fetch_commit.id();
            let mut reference = repo.find_reference(&format!("refs/heads/{}", branch_name))?;
            reference.set_target(target_oid, "fast-forward pull")?;
            repo.set_head(&format!("refs/heads/{}", branch_name))?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
            Ok(())
        } else {
            anyhow::bail!("Cannot fast-forward; manual merge required")
        }
    }

    /// Create remote callbacks for authentication
    fn create_callbacks() -> RemoteCallbacks<'static> {
        let mut callbacks = RemoteCallbacks::new();

        callbacks.credentials(|url, username_from_url, allowed_types| {
            // Try SSH agent first
            if allowed_types.contains(CredentialType::SSH_KEY) {
                if let Some(username) = username_from_url {
                    // Try SSH agent
                    if let Ok(cred) = Cred::ssh_key_from_agent(username) {
                        return Ok(cred);
                    }

                    // Try default SSH key locations
                    let home = dirs::home_dir().unwrap_or_default();
                    let ssh_dir = home.join(".ssh");

                    for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
                        let key_path = ssh_dir.join(key_name);
                        if key_path.exists() {
                            if let Ok(cred) =
                                Cred::ssh_key(username, None, &key_path, None)
                            {
                                return Ok(cred);
                            }
                        }
                    }
                }
            }

            // Try default credentials (for HTTPS with credential helper)
            if allowed_types.contains(CredentialType::DEFAULT) {
                if let Ok(cred) = Cred::default() {
                    return Ok(cred);
                }
            }

            // Try username/password from git credential helper
            if allowed_types.contains(CredentialType::USER_PASS_PLAINTEXT) {
                if let Ok(cred) = Cred::credential_helper(
                    &git2::Config::open_default().unwrap_or_else(|_| git2::Config::new().unwrap()),
                    url,
                    username_from_url,
                ) {
                    return Ok(cred);
                }
            }

            Err(git2::Error::from_str("No valid credentials found"))
        });

        callbacks
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
