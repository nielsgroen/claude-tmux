//! Application state and business logic
//!
//! This module contains the core application state machine:
//! - `App` struct: main application state
//! - Mode handling and transitions
//! - Session actions and execution
//! - Dialog flows (rename, new session, worktree, PR)

mod helpers;
mod mode;

use anyhow::Result;

use crate::git::{self, GitContext, PullRequestInfo};
use crate::scroll_state::ScrollState;
use crate::session::Session;
use crate::tmux::Tmux;

// Re-export types that are part of the public API
pub use mode::{
    CreatePullRequestField, Mode, NewSessionField, NewWorktreeField, SessionAction,
};

// Use helpers internally
use helpers::{default_worktree_path, expand_path, sanitize_for_session_name};

/// Main application state
pub struct App {
    /// All discovered sessions
    pub sessions: Vec<Session>,
    /// Currently selected index
    pub selected: usize,
    /// Current UI mode
    pub mode: Mode,
    /// Whether the app should quit
    pub should_quit: bool,
    /// Name of the currently attached session (if any)
    pub current_session: Option<String>,
    /// Filter text for filtering sessions
    pub filter: String,
    /// Error message to display (clears on next action)
    pub error: Option<String>,
    /// Success message to display (clears on next action)
    pub message: Option<String>,
    /// Cached preview content for the selected session's pane
    pub preview_content: Option<String>,
    /// Available actions for the selected session (computed when entering action menu)
    pub available_actions: Vec<SessionAction>,
    /// Currently highlighted action in ActionMenu mode
    pub selected_action: usize,
    /// Action pending confirmation
    pub pending_action: Option<SessionAction>,
    /// PR info for the selected session (computed when entering action menu)
    pub pr_info: Option<PullRequestInfo>,
    /// Scroll state for the session list
    pub scroll_state: ScrollState,
}

impl App {
    // =========================================================================
    // Initialization and core lifecycle
    // =========================================================================

    /// Create a new App instance
    pub fn new() -> Result<Self> {
        let sessions = Tmux::list_sessions()?;
        let current_session = Tmux::current_session()?;

        let mut app = Self {
            sessions,
            selected: 0,
            mode: Mode::Normal,
            should_quit: false,
            current_session,
            filter: String::new(),
            error: None,
            message: None,
            preview_content: None,
            available_actions: Vec::new(),
            selected_action: 0,
            pending_action: None,
            pr_info: None,
            scroll_state: ScrollState::new(),
        };

        app.update_preview();
        Ok(app)
    }

    /// Update the preview content for the currently selected session
    pub fn update_preview(&mut self) {
        const PREVIEW_LINES: usize = 15;

        let pane_id = self.selected_session().and_then(|session| {
            // Prefer Claude pane, fall back to first pane
            session
                .claude_code_pane
                .clone()
                .or_else(|| session.panes.first().map(|p| p.id.clone()))
        });

        self.preview_content = pane_id.and_then(|id| {
            // Don't strip empty lines - preserve visual layout for preview
            Tmux::capture_pane(&id, PREVIEW_LINES, false).ok()
        });
    }

    /// Clear any displayed messages
    pub fn clear_messages(&mut self) {
        self.error = None;
        self.message = None;
    }

    /// Refresh the session list (shows "Refreshed" message)
    pub fn refresh(&mut self) {
        self.clear_messages();
        if self.refresh_sessions() {
            self.message = Some("Refreshed".to_string());
        }
    }

    /// Refresh sessions without affecting messages (for use after git operations)
    fn refresh_sessions(&mut self) -> bool {
        match Tmux::list_sessions() {
            Ok(sessions) => {
                self.sessions = sessions;
                // Ensure selected index is still valid
                if self.selected >= self.sessions.len() && !self.sessions.is_empty() {
                    self.selected = self.sessions.len() - 1;
                }
                self.update_preview();
                true
            }
            Err(e) => {
                self.error = Some(format!("Failed to refresh: {}", e));
                false
            }
        }
    }

    // =========================================================================
    // Session selection and navigation
    // =========================================================================

    /// Get filtered sessions based on current filter
    pub fn filtered_sessions(&self) -> Vec<&Session> {
        if self.filter.is_empty() {
            self.sessions.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.sessions
                .iter()
                .filter(|s| {
                    s.name.to_lowercase().contains(&filter_lower)
                        || s.display_path().to_lowercase().contains(&filter_lower)
                })
                .collect()
        }
    }

    /// Get the currently selected session
    pub fn selected_session(&self) -> Option<&Session> {
        let filtered = self.filtered_sessions();
        filtered.get(self.selected).copied()
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        let count = self.filtered_sessions().len();
        if count > 0 && self.selected > 0 {
            self.selected -= 1;
            self.update_preview();
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        let count = self.filtered_sessions().len();
        if count > 0 && self.selected < count - 1 {
            self.selected += 1;
            self.update_preview();
        }
    }

    /// Switch to the selected session
    pub fn switch_to_selected(&mut self) {
        self.clear_messages();
        if let Some(session) = self.selected_session() {
            let name = session.name.clone();
            match Tmux::switch_to_session(&name) {
                Ok(_) => {
                    self.should_quit = true;
                }
                Err(e) => {
                    self.error = Some(format!("Failed to switch: {}", e));
                }
            }
        }
    }

    // =========================================================================
    // Action menu
    // =========================================================================

    /// Enter the action menu for the selected session
    pub fn enter_action_menu(&mut self) {
        self.clear_messages();
        if self.selected_session().is_some() {
            self.compute_actions();
            self.mode = Mode::ActionMenu;
        }
    }

    /// Move to next action in the action menu
    pub fn select_next_action(&mut self) {
        if !self.available_actions.is_empty() {
            self.selected_action = (self.selected_action + 1) % self.available_actions.len();
        }
    }

    /// Move to previous action in the action menu
    pub fn select_prev_action(&mut self) {
        if !self.available_actions.is_empty() {
            if self.selected_action == 0 {
                self.selected_action = self.available_actions.len() - 1;
            } else {
                self.selected_action -= 1;
            }
        }
    }

    /// Execute the currently selected action from the action menu
    pub fn execute_selected_action(&mut self) {
        if let Some(action) = self.available_actions.get(self.selected_action).cloned() {
            if action.requires_confirmation() {
                self.pending_action = Some(action);
                self.mode = Mode::ConfirmAction;
            } else {
                // execute_action handles its own mode transitions
                self.execute_action(action);
            }
        }
    }

    /// Compute available actions for the selected session
    fn compute_actions(&mut self) {
        // Extract data we need from the session first to avoid borrow conflicts
        let session_data = self.selected_session().map(|s| {
            (s.working_directory.clone(), s.git_context.clone())
        });

        let Some((working_dir, git_context)) = session_data else {
            self.available_actions = vec![];
            self.pr_info = None;
            return;
        };

        let mut actions = vec![SessionAction::SwitchTo, SessionAction::Rename];

        // Reset PR info
        self.pr_info = None;

        // Add git actions if applicable
        if let Some(ref git) = git_context {
            // New worktree: available for any git repo
            actions.push(SessionAction::NewWorktree);

            // Stage: if there are unstaged changes
            if git.has_unstaged {
                actions.push(SessionAction::Stage);
            }
            // Commit: if there are staged changes
            if git.has_staged {
                actions.push(SessionAction::Commit);
            }

            // Fetch: always available if there's a remote (safe operation)
            if git.has_remote {
                actions.push(SessionAction::Fetch);
            }

            if git.has_upstream {
                // Push: ahead > 0 (dirty state doesn't prevent pushing commits)
                if git.ahead > 0 {
                    actions.push(SessionAction::Push);
                }
                // Pull: behind > 0 and clean (dirty state can cause merge conflicts)
                if git.behind > 0 && !git.is_dirty() {
                    actions.push(SessionAction::Pull);
                }

                // PR actions: upstream exists, gh available, GitHub remote, not on default branch
                if git::is_gh_available() && git::is_github_remote(&working_dir) {
                    // Check if not on default branch
                    if let Some(default_branch) = git::get_default_branch(&working_dir) {
                        if git.branch != default_branch {
                            // Check if PR already exists for this branch
                            let pr_info = git::get_pull_request_info(&working_dir);
                            if let Some(ref info) = pr_info {
                                if info.state == "OPEN" {
                                    actions.push(SessionAction::ViewPullRequest);
                                    actions.push(SessionAction::ClosePullRequest);
                                    actions.push(SessionAction::MergePullRequest);
                                    actions.push(SessionAction::MergePullRequestAndClose);
                                } else {
                                    // PR exists but is CLOSED or MERGED - can create a new one
                                    actions.push(SessionAction::CreatePullRequest);
                                }
                            } else {
                                // No PR exists, offer to create one
                                actions.push(SessionAction::CreatePullRequest);
                            }
                            // Store PR info for UI display
                            self.pr_info = pr_info;
                        }
                    }
                }
            } else if git.has_remote {
                // No upstream but remote exists - offer to push and set upstream
                actions.push(SessionAction::PushSetUpstream);
            }
        }

        actions.push(SessionAction::Kill);

        // Add worktree deletion option if this is a worktree
        if let Some(ref git) = git_context {
            if git.is_worktree {
                actions.push(SessionAction::KillAndDeleteWorktree);
            }
        }

        self.available_actions = actions;
        self.selected_action = 0;
    }

    // =========================================================================
    // Action execution
    // =========================================================================

    /// Start the kill confirmation flow (direct kill without action menu)
    pub fn start_kill(&mut self) {
        self.clear_messages();
        if self.selected_session().is_some() {
            self.pending_action = Some(SessionAction::Kill);
            self.mode = Mode::ConfirmAction;
        }
    }

    /// Confirm and execute the pending action
    pub fn confirm_action(&mut self) {
        if let Some(action) = self.pending_action.take() {
            self.execute_action(action);
        }
        self.mode = Mode::Normal;
    }

    /// Execute an action on the selected session
    fn execute_action(&mut self, action: SessionAction) {
        let Some(session) = self.selected_session() else {
            self.mode = Mode::Normal;
            return;
        };
        let session_name = session.name.clone();

        match action {
            SessionAction::SwitchTo => {
                match Tmux::switch_to_session(&session_name) {
                    Ok(_) => self.should_quit = true,
                    Err(e) => self.error = Some(format!("Failed to switch: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Rename => {
                self.mode = Mode::Rename {
                    old_name: session_name.clone(),
                    new_name: session_name,
                };
            }
            SessionAction::Stage => {
                let path = session.working_directory.clone();
                match GitContext::stage_all(&path) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Staged all changes".to_string());
                    }
                    Err(e) => self.error = Some(format!("Stage failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Commit => {
                self.mode = Mode::Commit {
                    message: String::new(),
                };
            }
            SessionAction::Push => {
                let path = session.working_directory.clone();
                match GitContext::push(&path) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Pushed to remote".to_string());
                    }
                    Err(e) => self.error = Some(format!("Push failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::PushSetUpstream => {
                let path = session.working_directory.clone();
                match GitContext::push_set_upstream(&path) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Pushed and set upstream".to_string());
                    }
                    Err(e) => self.error = Some(format!("Push failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Fetch => {
                let path = session.working_directory.clone();
                match GitContext::fetch(&path) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Fetched from remote".to_string());
                    }
                    Err(e) => self.error = Some(format!("Fetch failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Pull => {
                let path = session.working_directory.clone();
                match GitContext::pull(&path) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Pulled from remote".to_string());
                    }
                    Err(e) => self.error = Some(format!("Pull failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::CreatePullRequest => {
                self.start_create_pull_request();
            }
            SessionAction::ViewPullRequest => {
                let path = session.working_directory.clone();
                match git::view_pull_request(&path) {
                    Ok(_) => {
                        self.message = Some("Opened PR in browser".to_string());
                    }
                    Err(e) => self.error = Some(format!("Failed to open PR: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::ClosePullRequest => {
                let path = session.working_directory.clone();
                match git::close_pull_request(&path) {
                    Ok(_) => {
                        self.message = Some("Closed pull request".to_string());
                    }
                    Err(e) => self.error = Some(format!("Failed to close PR: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::MergePullRequest => {
                let path = session.working_directory.clone();
                match git::merge_pull_request(&path, false) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Merged pull request".to_string());
                    }
                    Err(e) => self.error = Some(format!("Failed to merge PR: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::MergePullRequestAndClose => {
                let path = session.working_directory.clone();
                let is_worktree = session
                    .git_context
                    .as_ref()
                    .map(|g| g.is_worktree)
                    .unwrap_or(false);

                // Step 1: Merge PR
                match git::merge_pull_request(&path, false) {
                    Ok(_) => {
                        // Step 2: Delete worktree if applicable
                        if is_worktree {
                            if let Err(e) = GitContext::delete_worktree(&path, true) {
                                self.error =
                                    Some(format!("PR merged but failed to delete worktree: {}", e));
                                self.mode = Mode::Normal;
                                return;
                            }
                        }

                        // Step 3: Kill the session
                        match Tmux::kill_session(&session_name) {
                            Ok(_) => {
                                self.refresh_sessions();
                                self.message = Some(if is_worktree {
                                    "Merged PR, removed worktree, and closed session".to_string()
                                } else {
                                    "Merged PR and closed session".to_string()
                                });
                            }
                            Err(e) => {
                                self.refresh_sessions();
                                self.error = Some(format!(
                                    "PR merged but failed to kill session: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Err(e) => self.error = Some(format!("Failed to merge PR: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Kill => {
                match Tmux::kill_session(&session_name) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some(format!("Killed session '{}'", session_name));
                    }
                    Err(e) => self.error = Some(format!("Failed to kill: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::NewWorktree => {
                self.start_new_worktree();
            }
            SessionAction::KillAndDeleteWorktree => {
                let worktree_path = session.working_directory.clone();
                // First delete the worktree (while session still provides git context)
                match GitContext::delete_worktree(&worktree_path, false) {
                    Ok(_) => {
                        // Then kill the session
                        match Tmux::kill_session(&session_name) {
                            Ok(_) => {
                                self.refresh_sessions();
                                self.message = Some(format!(
                                    "Deleted worktree and killed session '{}'",
                                    session_name
                                ));
                            }
                            Err(e) => {
                                self.refresh_sessions();
                                self.error = Some(format!(
                                    "Worktree deleted but failed to kill session: {}",
                                    e
                                ));
                            }
                        }
                    }
                    Err(e) => self.error = Some(format!("Failed to delete worktree: {}", e)),
                }
                self.mode = Mode::Normal;
            }
        }
    }

    // =========================================================================
    // Dialog flows: Rename
    // =========================================================================

    /// Start the rename flow
    pub fn start_rename(&mut self) {
        self.clear_messages();
        if let Some(session) = self.selected_session() {
            self.mode = Mode::Rename {
                old_name: session.name.clone(),
                new_name: session.name.clone(),
            };
        }
    }

    /// Confirm and execute session rename
    pub fn confirm_rename(&mut self) {
        if let Mode::Rename {
            ref old_name,
            ref new_name,
        } = self.mode
        {
            let old = old_name.clone();
            let new = new_name.clone();

            if old == new {
                self.mode = Mode::Normal;
                return;
            }

            match Tmux::rename_session(&old, &new) {
                Ok(_) => {
                    self.refresh_sessions();
                    self.message = Some(format!("Renamed '{}' to '{}'", old, new));
                }
                Err(e) => {
                    self.error = Some(format!("Failed to rename: {}", e));
                }
            }
        }
        self.mode = Mode::Normal;
    }

    // =========================================================================
    // Dialog flows: Commit
    // =========================================================================

    /// Confirm and execute the commit
    pub fn confirm_commit(&mut self) {
        if let Mode::Commit { ref message } = self.mode {
            if message.trim().is_empty() {
                self.error = Some("Commit message cannot be empty".to_string());
                self.mode = Mode::Normal;
                return;
            }

            if let Some(session) = self.selected_session() {
                let path = session.working_directory.clone();
                let msg = message.clone();
                match GitContext::commit(&path, &msg) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some("Committed changes".to_string());
                    }
                    Err(e) => self.error = Some(format!("Commit failed: {}", e)),
                }
            }
        }
        self.mode = Mode::Normal;
    }

    // =========================================================================
    // Dialog flows: New Session
    // =========================================================================

    /// Start the new session flow
    pub fn start_new_session(&mut self) {
        self.clear_messages();
        // Default to current directory
        let default_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "~".to_string());

        // Get initial path suggestions
        let completion = crate::completion::complete_path(&default_path);

        self.mode = Mode::NewSession {
            name: String::new(),
            path: default_path,
            field: NewSessionField::Name,
            path_suggestions: completion.suggestions,
            path_selected: None,
        };
    }

    /// Create the new session
    pub fn confirm_new_session(&mut self, start_claude: bool) {
        if let Mode::NewSession {
            ref name, ref path, ..
        } = self.mode
        {
            if name.is_empty() {
                self.error = Some("Session name cannot be empty".to_string());
                self.mode = Mode::Normal;
                return;
            }

            let session_name = name.clone();
            let session_path = expand_path(path);

            match Tmux::new_session(&session_name, &session_path, start_claude) {
                Ok(_) => {
                    self.refresh_sessions();
                    self.message = Some(format!("Created session '{}'", session_name));
                }
                Err(e) => {
                    self.error = Some(format!("Failed to create session: {}", e));
                }
            }
        }
        self.mode = Mode::Normal;
    }

    // =========================================================================
    // Dialog flows: New Worktree
    // =========================================================================

    /// Start the new worktree flow
    pub fn start_new_worktree(&mut self) {
        self.clear_messages();
        let Some(session) = self.selected_session() else {
            return;
        };

        // Get the repo path (use main repo if this is a worktree)
        let source_repo = if let Some(ref git) = session.git_context {
            if git.is_worktree {
                git.main_repo_path
                    .clone()
                    .unwrap_or_else(|| session.working_directory.clone())
            } else {
                session.working_directory.clone()
            }
        } else {
            return; // Not a git repo
        };

        // Get list of branches
        let all_branches = match GitContext::list_branches(&source_repo) {
            Ok(branches) => branches,
            Err(e) => {
                self.error = Some(format!("Failed to list branches: {}", e));
                return;
            }
        };

        self.mode = Mode::NewWorktree {
            source_repo,
            all_branches,
            branch_input: String::new(),
            selected_branch: None,
            worktree_path: String::new(),
            session_name: String::new(),
            field: NewWorktreeField::Branch,
            path_suggestions: Vec::new(),
            path_selected: None,
        };
    }

    /// Get filtered branches based on current input
    pub fn filtered_branches(&self) -> Vec<&str> {
        if let Mode::NewWorktree {
            ref all_branches,
            ref branch_input,
            ..
        } = self.mode
        {
            if branch_input.is_empty() {
                all_branches.iter().map(|s| s.as_str()).collect()
            } else {
                let input_lower = branch_input.to_lowercase();
                all_branches
                    .iter()
                    .filter(|b| b.to_lowercase().contains(&input_lower))
                    .map(|s| s.as_str())
                    .collect()
            }
        } else {
            vec![]
        }
    }

    /// Update suggestions when branch input changes
    pub fn update_worktree_suggestions(&mut self) {
        if let Mode::NewWorktree {
            ref source_repo,
            ref all_branches,
            ref branch_input,
            ref mut selected_branch,
            ref mut worktree_path,
            ref mut session_name,
            ..
        } = self.mode
        {
            // Filter branches
            let filtered: Vec<&str> = if branch_input.is_empty() {
                all_branches.iter().map(|s| s.as_str()).collect()
            } else {
                let input_lower = branch_input.to_lowercase();
                all_branches
                    .iter()
                    .filter(|b| b.to_lowercase().contains(&input_lower))
                    .map(|s| s.as_str())
                    .collect()
            };

            // Update selected branch
            if filtered.is_empty() {
                *selected_branch = None;
            } else if let Some(idx) = *selected_branch {
                if idx >= filtered.len() {
                    *selected_branch = Some(filtered.len() - 1);
                }
            }

            // Auto-update path and session name based on branch input
            let branch_for_path = if let Some(idx) = *selected_branch {
                filtered.get(idx).copied().unwrap_or(branch_input.as_str())
            } else {
                branch_input.as_str()
            };

            if !branch_for_path.is_empty() {
                *worktree_path = default_worktree_path(source_repo, branch_for_path)
                    .to_string_lossy()
                    .to_string();
                // Session name: repo-name + branch suffix
                let repo_name = source_repo
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("repo");
                let branch_suffix = sanitize_for_session_name(branch_for_path);
                *session_name = format!("{}-{}", repo_name, branch_suffix);
            }
        }
    }

    /// Create the new worktree and session
    pub fn confirm_new_worktree(&mut self) {
        let (source_repo, all_branches, branch_input, selected_branch, worktree_path, session_name) =
            if let Mode::NewWorktree {
                ref source_repo,
                ref all_branches,
                ref branch_input,
                selected_branch,
                ref worktree_path,
                ref session_name,
                ..
            } = self.mode
            {
                (
                    source_repo.clone(),
                    all_branches.clone(),
                    branch_input.clone(),
                    selected_branch,
                    worktree_path.clone(),
                    session_name.clone(),
                )
            } else {
                return;
            };

        // Validate inputs
        if branch_input.is_empty() && selected_branch.is_none() {
            self.error = Some("Branch name cannot be empty".to_string());
            self.mode = Mode::Normal;
            return;
        }

        if session_name.is_empty() {
            self.error = Some("Session name cannot be empty".to_string());
            self.mode = Mode::Normal;
            return;
        }

        if worktree_path.is_empty() {
            self.error = Some("Worktree path cannot be empty".to_string());
            self.mode = Mode::Normal;
            return;
        }

        // Determine if this is a new branch or existing
        let filtered: Vec<&str> = if branch_input.is_empty() {
            all_branches.iter().map(|s| s.as_str()).collect()
        } else {
            let input_lower = branch_input.to_lowercase();
            all_branches
                .iter()
                .filter(|b| b.to_lowercase().contains(&input_lower))
                .map(|s| s.as_str())
                .collect()
        };

        let (branch_name, is_new_branch) = if let Some(idx) = selected_branch {
            // User selected an existing branch
            (
                filtered
                    .get(idx)
                    .copied()
                    .unwrap_or(&branch_input)
                    .to_string(),
                false,
            )
        } else if all_branches.iter().any(|b| b == &branch_input) {
            // Exact match with existing branch
            (branch_input.clone(), false)
        } else {
            // New branch
            (branch_input.clone(), true)
        };

        let worktree_path_buf = expand_path(&worktree_path);

        // Create the worktree
        match GitContext::create_worktree(
            &source_repo,
            &worktree_path_buf,
            &branch_name,
            is_new_branch,
        ) {
            Ok(_) => {
                // Create the session
                match Tmux::new_session(&session_name, &worktree_path_buf, true) {
                    Ok(_) => {
                        self.refresh_sessions();
                        self.message = Some(format!(
                            "Created worktree '{}' and session '{}'",
                            branch_name, session_name
                        ));
                    }
                    Err(e) => {
                        self.error = Some(format!(
                            "Worktree created but session creation failed: {}",
                            e
                        ));
                    }
                }
            }
            Err(e) => {
                self.error = Some(format!("Failed to create worktree: {}", e));
            }
        }

        self.mode = Mode::Normal;
    }

    // =========================================================================
    // Dialog flows: Create Pull Request
    // =========================================================================

    /// Start the create pull request flow
    pub fn start_create_pull_request(&mut self) {
        self.clear_messages();
        let Some(session) = self.selected_session() else {
            return;
        };

        let path = &session.working_directory;
        let base_branch = git::get_default_branch(path).unwrap_or_else(|| "main".to_string());

        self.mode = Mode::CreatePullRequest {
            title: String::new(),
            body: String::new(),
            base_branch,
            field: CreatePullRequestField::Title,
        };
    }

    /// Confirm and execute PR creation
    pub fn confirm_create_pull_request(&mut self) {
        let (title, body, base_branch) = if let Mode::CreatePullRequest {
            ref title,
            ref body,
            ref base_branch,
            ..
        } = self.mode
        {
            (title.clone(), body.clone(), base_branch.clone())
        } else {
            self.mode = Mode::Normal;
            return;
        };

        if title.trim().is_empty() {
            self.error = Some("PR title cannot be empty".to_string());
            self.mode = Mode::Normal;
            return;
        }

        if let Some(session) = self.selected_session() {
            let path = session.working_directory.clone();
            match git::create_pull_request(&path, &title, &body, &base_branch) {
                Ok(result) => {
                    self.message = Some(format!("Created PR: {}", result.url));
                }
                Err(e) => {
                    self.error = Some(format!("Failed to create PR: {}", e));
                }
            }
        }

        self.mode = Mode::Normal;
    }

    // =========================================================================
    // Filter mode
    // =========================================================================

    /// Start filter mode
    pub fn start_filter(&mut self) {
        self.clear_messages();
        self.mode = Mode::Filter {
            input: self.filter.clone(),
        };
    }

    /// Apply filter and return to normal mode
    pub fn apply_filter(&mut self) {
        if let Mode::Filter { ref input } = self.mode {
            self.filter = input.clone();
            self.selected = 0; // Reset selection when filter changes
        }
        self.mode = Mode::Normal;
        self.update_preview();
    }

    /// Clear the filter
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.selected = 0;
    }

    /// Show help
    pub fn show_help(&mut self) {
        self.clear_messages();
        self.mode = Mode::Help;
    }

    /// Cancel current mode and return to normal
    pub fn cancel(&mut self) {
        self.pending_action = None;
        self.pr_info = None;
        self.mode = Mode::Normal;
    }

    // =========================================================================
    // Status and statistics
    // =========================================================================

    /// Count sessions by status
    pub fn status_counts(&self) -> (usize, usize, usize) {
        use crate::session::ClaudeCodeStatus;

        let mut working = 0;
        let mut waiting = 0;
        let mut idle = 0;

        for session in &self.sessions {
            match session.claude_code_status {
                ClaudeCodeStatus::Working => working += 1,
                ClaudeCodeStatus::WaitingInput => waiting += 1,
                ClaudeCodeStatus::Idle => idle += 1,
                ClaudeCodeStatus::Unknown => {}
            }
        }

        (working, waiting, idle)
    }

    // =========================================================================
    // Path completion methods
    // =========================================================================

    /// Update path suggestions for NewSession mode
    pub fn update_new_session_path_suggestions(&mut self) {
        if let Mode::NewSession {
            ref path,
            ref mut path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            let completion = crate::completion::complete_path(path);
            *path_suggestions = completion.suggestions;
            // Reset selection if it's out of bounds
            if let Some(idx) = *path_selected {
                if idx >= path_suggestions.len() {
                    *path_selected = if path_suggestions.is_empty() {
                        None
                    } else {
                        Some(path_suggestions.len() - 1)
                    };
                }
            }
        }
    }

    /// Update path suggestions for NewWorktree mode
    pub fn update_worktree_path_suggestions(&mut self) {
        if let Mode::NewWorktree {
            ref worktree_path,
            ref mut path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            let completion = crate::completion::complete_path(worktree_path);
            *path_suggestions = completion.suggestions;
            // Reset selection if it's out of bounds
            if let Some(idx) = *path_selected {
                if idx >= path_suggestions.len() {
                    *path_selected = if path_suggestions.is_empty() {
                        None
                    } else {
                        Some(path_suggestions.len() - 1)
                    };
                }
            }
        }
    }

    /// Select previous path suggestion in NewSession mode
    pub fn select_prev_new_session_path(&mut self) {
        if let Mode::NewSession {
            ref path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            if path_suggestions.is_empty() {
                return;
            }
            *path_selected = Some(
                path_selected
                    .map(|i| {
                        if i == 0 {
                            path_suggestions.len() - 1
                        } else {
                            i - 1
                        }
                    })
                    .unwrap_or(path_suggestions.len() - 1),
            );
        }
    }

    /// Select next path suggestion in NewSession mode
    pub fn select_next_new_session_path(&mut self) {
        if let Mode::NewSession {
            ref path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            if path_suggestions.is_empty() {
                return;
            }
            *path_selected = Some(
                path_selected
                    .map(|i| (i + 1) % path_suggestions.len())
                    .unwrap_or(0),
            );
        }
    }

    /// Accept the current path completion in NewSession mode
    pub fn accept_new_session_path_completion(&mut self) {
        if let Mode::NewSession {
            ref mut path,
            ref path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            // If a suggestion is selected, use it
            if let Some(idx) = *path_selected {
                if let Some(suggestion) = path_suggestions.get(idx) {
                    *path = suggestion.clone();
                    *path_selected = None;
                }
            } else if let Some(first) = path_suggestions.first() {
                // Otherwise use the first suggestion (ghost text)
                *path = first.clone();
            }
        }
        // Update suggestions after accepting
        self.update_new_session_path_suggestions();
    }

    /// Select previous path suggestion in NewWorktree mode
    pub fn select_prev_worktree_path(&mut self) {
        if let Mode::NewWorktree {
            ref path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            if path_suggestions.is_empty() {
                return;
            }
            *path_selected = Some(
                path_selected
                    .map(|i| {
                        if i == 0 {
                            path_suggestions.len() - 1
                        } else {
                            i - 1
                        }
                    })
                    .unwrap_or(path_suggestions.len() - 1),
            );
        }
    }

    /// Select next path suggestion in NewWorktree mode
    pub fn select_next_worktree_path(&mut self) {
        if let Mode::NewWorktree {
            ref path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            if path_suggestions.is_empty() {
                return;
            }
            *path_selected = Some(
                path_selected
                    .map(|i| (i + 1) % path_suggestions.len())
                    .unwrap_or(0),
            );
        }
    }

    /// Accept the current path completion in NewWorktree mode
    pub fn accept_worktree_path_completion(&mut self) {
        if let Mode::NewWorktree {
            ref mut worktree_path,
            ref path_suggestions,
            ref mut path_selected,
            ..
        } = self.mode
        {
            // If a suggestion is selected, use it
            if let Some(idx) = *path_selected {
                if let Some(suggestion) = path_suggestions.get(idx) {
                    *worktree_path = suggestion.clone();
                    *path_selected = None;
                }
            } else if let Some(first) = path_suggestions.first() {
                // Otherwise use the first suggestion (ghost text)
                *worktree_path = first.clone();
            }
        }
        // Update suggestions after accepting
        self.update_worktree_path_suggestions();
    }

    /// Accept the current branch completion in NewWorktree mode
    pub fn accept_branch_completion(&mut self) {
        let selected_branch_name = if let Mode::NewWorktree {
            ref all_branches,
            ref branch_input,
            selected_branch,
            ..
        } = self.mode
        {
            // Get filtered branches
            let filtered: Vec<&str> = if branch_input.is_empty() {
                all_branches.iter().map(|s| s.as_str()).collect()
            } else {
                let input_lower = branch_input.to_lowercase();
                all_branches
                    .iter()
                    .filter(|b| b.to_lowercase().contains(&input_lower))
                    .map(|s| s.as_str())
                    .collect()
            };

            // Get the branch to accept
            if let Some(idx) = selected_branch {
                filtered.get(idx).map(|s| s.to_string())
            } else {
                filtered.first().map(|s| s.to_string())
            }
        } else {
            None
        };

        // Now update the branch_input with the selected branch
        if let Some(branch_name) = selected_branch_name {
            if let Mode::NewWorktree {
                ref mut branch_input,
                ref mut selected_branch,
                ..
            } = self.mode
            {
                *branch_input = branch_name;
                *selected_branch = None;
            }
            self.update_worktree_suggestions();
        }
    }

    // =========================================================================
    // Scroll/list computation
    // =========================================================================

    /// Compute the flat list index for the current selection.
    ///
    /// The list has a complex structure where the selected session expands
    /// to show metadata and action items. This method computes the index
    /// into the flat list of rendered items.
    pub fn compute_flat_list_index(&self) -> usize {
        let filtered_count = self.filtered_sessions().len();
        if filtered_count == 0 {
            return 0;
        }

        match self.mode {
            Mode::ActionMenu => {
                // Count items before selected session (1 row each)
                let mut index = self.selected;

                // Add 1 for the selected session row itself
                index += 1;

                // Add 1 for metadata row (always present when expanded)
                index += 1;

                // Add 1 for git info row if present
                if self
                    .selected_session()
                    .is_some_and(|s| s.git_context.is_some())
                {
                    index += 1;

                    // Add 1 for PR info row if present
                    if self.pr_info.is_some() {
                        index += 1;
                    }
                }

                // Add 1 for separator
                index += 1;

                // Add selected_action to get to the highlighted action
                index += self.selected_action;

                index
            }
            _ => {
                // In non-ActionMenu modes, just the session index
                self.selected
            }
        }
    }

    /// Compute the total number of items in the rendered list.
    ///
    /// This accounts for the expanded content when in ActionMenu mode.
    pub fn compute_total_list_items(&self) -> usize {
        let filtered_count = self.filtered_sessions().len();
        if filtered_count == 0 {
            return 0;
        }

        match self.mode {
            Mode::ActionMenu => {
                // Base: one row per session
                let mut total = filtered_count;

                // Add expanded content for selected session:
                // - 1 metadata row
                // - 1 git info row (if git context)
                // - 1 PR info row (if pr_info)
                // - 1 separator
                // - N action rows
                // - 1 end separator
                total += 1; // metadata row

                if self
                    .selected_session()
                    .is_some_and(|s| s.git_context.is_some())
                {
                    total += 1; // git info row
                    if self.pr_info.is_some() {
                        total += 1; // PR info row
                    }
                }

                total += 1; // separator
                total += self.available_actions.len(); // action rows
                total += 1; // end separator

                total
            }
            _ => filtered_count,
        }
    }
}
