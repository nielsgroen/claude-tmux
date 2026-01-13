use std::path::PathBuf;

use anyhow::Result;

use crate::git::{self, GitContext};
use crate::session::Session;
use crate::tmux::Tmux;

/// The current mode/state of the application
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Normal browsing mode
    Normal,
    /// Viewing actions for selected session
    ActionMenu,
    /// Filtering sessions with search input
    Filter { input: String },
    /// Confirming an action (kill, etc.)
    ConfirmAction,
    /// Creating a new session
    NewSession { name: String, path: String, field: NewSessionField },
    /// Renaming a session
    Rename { old_name: String, new_name: String },
    /// Entering commit message
    Commit { message: String },
    /// Creating a new session from a worktree
    NewWorktree {
        /// The source repository path (from selected session)
        source_repo: PathBuf,
        /// All branches in the repository
        all_branches: Vec<String>,
        /// Branch name input (may be new or existing)
        branch_input: String,
        /// Selected index in filtered branches (None = creating new branch)
        selected_branch: Option<usize>,
        /// Worktree path
        worktree_path: String,
        /// Session name
        session_name: String,
        /// Which field is active
        field: NewWorktreeField,
    },
    /// Creating a pull request
    CreatePullRequest {
        /// PR title
        title: String,
        /// PR body/description
        body: String,
        /// Base branch to merge into
        base_branch: String,
        /// Which field is active
        field: CreatePullRequestField,
    },
    /// Showing help
    Help,
}

/// An action that can be performed on a session
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAction {
    /// Switch to this session
    SwitchTo,
    /// Rename this session
    Rename,
    /// Create a new session from a worktree
    NewWorktree,
    /// Stage all changes
    Stage,
    /// Commit staged changes
    Commit,
    /// Push commits to remote
    Push,
    /// Push and set upstream branch
    PushSetUpstream,
    /// Pull commits from remote
    Pull,
    /// Create a pull request
    CreatePullRequest,
    /// Kill this session
    Kill,
    /// Kill session and delete its worktree
    KillAndDeleteWorktree,
}

impl SessionAction {
    /// Returns the display label for this action
    pub fn label(&self) -> &'static str {
        match self {
            Self::SwitchTo => "Switch to session",
            Self::Rename => "Rename session",
            Self::NewWorktree => "New session from worktree",
            Self::Stage => "Stage all changes",
            Self::Commit => "Commit staged changes",
            Self::Push => "Push to remote",
            Self::PushSetUpstream => "Push and set upstream",
            Self::Pull => "Pull from remote",
            Self::CreatePullRequest => "Create pull request",
            Self::Kill => "Kill session",
            Self::KillAndDeleteWorktree => "Kill session + delete worktree",
        }
    }

    /// Whether this action requires confirmation
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::Kill | Self::KillAndDeleteWorktree)
    }
}

/// Which field is active in the new session dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewSessionField {
    Name,
    Path,
}

/// Which field is active in the new worktree dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWorktreeField {
    Branch,
    Path,
    SessionName,
}

/// Which field is active in the create pull request dialog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatePullRequestField {
    Title,
    Body,
    BaseBranch,
}

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
}

impl App {
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
                // Enter rename mode (don't set Normal)
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
                // Enter commit mode (don't set Normal)
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
                // Enter create PR mode
                self.start_create_pull_request();
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
                // Enter new worktree mode
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

    /// Start the new session flow
    pub fn start_new_session(&mut self) {
        self.clear_messages();
        // Default to current directory
        let default_path = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "~".to_string());

        self.mode = Mode::NewSession {
            name: String::new(),
            path: default_path,
            field: NewSessionField::Name,
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
            (filtered.get(idx).copied().unwrap_or(&branch_input).to_string(), false)
        } else if all_branches.iter().any(|b| b == &branch_input) {
            // Exact match with existing branch
            (branch_input.clone(), false)
        } else {
            // New branch
            (branch_input.clone(), true)
        };

        let worktree_path_buf = expand_path(&worktree_path);

        // Create the worktree
        match GitContext::create_worktree(&source_repo, &worktree_path_buf, &branch_name, is_new_branch)
        {
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

    /// Compute available actions for the selected session
    fn compute_actions(&mut self) {
        let Some(session) = self.selected_session() else {
            self.available_actions = vec![];
            return;
        };

        let mut actions = vec![
            SessionAction::SwitchTo,
            SessionAction::Rename,
        ];

        // Add git actions if applicable
        if let Some(ref git) = session.git_context {
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

            if git.has_upstream {
                // Push: ahead > 0 (dirty state doesn't prevent pushing commits)
                if git.ahead > 0 {
                    actions.push(SessionAction::Push);
                }
                // Pull: behind > 0 and clean (dirty state can cause merge conflicts)
                if git.behind > 0 && !git.is_dirty() {
                    actions.push(SessionAction::Pull);
                }

                // Create PR: upstream exists, gh available, GitHub remote, not on default branch
                let path = &session.working_directory;
                if git::is_gh_available() && git::is_github_remote(path) {
                    // Check if not on default branch
                    if let Some(default_branch) = git::get_default_branch(path) {
                        if git.branch != default_branch {
                            actions.push(SessionAction::CreatePullRequest);
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
        if let Some(ref git) = session.git_context {
            if git.is_worktree {
                actions.push(SessionAction::KillAndDeleteWorktree);
            }
        }

        self.available_actions = actions;
        self.selected_action = 0;
    }

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
                // (e.g., Rename sets Mode::Rename, SwitchTo quits)
                self.execute_action(action);
            }
        }
    }

    /// Cancel current mode and return to normal
    pub fn cancel(&mut self) {
        self.pending_action = None;
        self.mode = Mode::Normal;
    }

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
}

/// Expand ~ to home directory in a path string
fn expand_path(path: &str) -> PathBuf {
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
fn sanitize_for_session_name(branch: &str) -> String {
    branch
        .rsplit('/')
        .next()
        .unwrap_or(branch)
        .replace(['/', '\\', ' ', ':', '.'], "-")
}

/// Generate default worktree path from repo path and branch name
/// e.g., ~/repos/project + feature/foo -> ~/repos/project-foo
fn default_worktree_path(repo_path: &std::path::Path, branch: &str) -> PathBuf {
    let parent = repo_path.parent().unwrap_or(repo_path);
    let repo_name = repo_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");
    let branch_suffix = sanitize_for_session_name(branch);
    parent.join(format!("{}-{}", repo_name, branch_suffix))
}
