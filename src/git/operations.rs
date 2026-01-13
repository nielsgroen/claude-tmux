//! Core git operations using libgit2
//!
//! Provides stage, commit, push, pull, and fetch operations.

use std::path::Path;

use anyhow::{Context, Result};
use git2::{
    AutotagOption, Cred, CredentialType, FetchOptions, PushOptions, RemoteCallbacks, Repository,
};

use super::GitContext;

impl GitContext {
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

        let callbacks = create_callbacks();
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

        let callbacks = create_callbacks();
        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(callbacks);

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);

        remote
            .push(&[&refspec], Some(&mut push_options))
            .context("Push failed")?;

        Ok(())
    }

    /// Fetch from the remote without merging (updates remote tracking branches)
    pub fn fetch(path: &Path) -> Result<()> {
        let repo = Repository::discover(path).context("Failed to open repository")?;

        // Find the first remote (usually "origin")
        let remotes = repo.remotes().context("Failed to list remotes")?;
        let remote_name = remotes
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("No remotes configured"))?;

        let mut remote = repo
            .find_remote(remote_name)
            .context("Failed to find remote")?;

        let callbacks = create_callbacks();
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(callbacks);
        fetch_options.download_tags(AutotagOption::Auto);

        // Fetch all branches from the remote
        remote
            .fetch(&[] as &[&str], Some(&mut fetch_options), None)
            .context("Fetch failed")?;

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
        let callbacks = create_callbacks();
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
                        if let Ok(cred) = Cred::ssh_key(username, None, &key_path, None) {
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
