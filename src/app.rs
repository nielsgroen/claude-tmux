use std::path::PathBuf;

use anyhow::Result;

use crate::git::GitContext;
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
    /// Push commits to remote
    Push,
    /// Push and set upstream branch
    PushSetUpstream,
    /// Pull commits from remote
    Pull,
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
            Self::Push => "Push to remote",
            Self::PushSetUpstream => "Push and set upstream",
            Self::Pull => "Pull from remote",
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

    /// Refresh the session list
    pub fn refresh(&mut self) {
        self.clear_messages();
        match Tmux::list_sessions() {
            Ok(sessions) => {
                self.sessions = sessions;
                // Ensure selected index is still valid
                if self.selected >= self.sessions.len() && !self.sessions.is_empty() {
                    self.selected = self.sessions.len() - 1;
                }
                self.update_preview();
                self.message = Some("Refreshed".to_string());
            }
            Err(e) => {
                self.error = Some(format!("Failed to refresh: {}", e));
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
            SessionAction::Push => {
                let path = session.working_directory.clone();
                match GitContext::push(&path) {
                    Ok(_) => {
                        self.message = Some("Pushed to remote".to_string());
                        self.refresh();
                    }
                    Err(e) => self.error = Some(format!("Push failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::PushSetUpstream => {
                let path = session.working_directory.clone();
                match GitContext::push_set_upstream(&path) {
                    Ok(_) => {
                        self.message = Some("Pushed and set upstream".to_string());
                        self.refresh();
                    }
                    Err(e) => self.error = Some(format!("Push failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Pull => {
                let path = session.working_directory.clone();
                match GitContext::pull(&path) {
                    Ok(_) => {
                        self.message = Some("Pulled from remote".to_string());
                        self.refresh();
                    }
                    Err(e) => self.error = Some(format!("Pull failed: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::Kill => {
                match Tmux::kill_session(&session_name) {
                    Ok(_) => {
                        self.message = Some(format!("Killed session '{}'", session_name));
                        self.refresh();
                    }
                    Err(e) => self.error = Some(format!("Failed to kill: {}", e)),
                }
                self.mode = Mode::Normal;
            }
            SessionAction::KillAndDeleteWorktree => {
                // For now, just kill the session (worktree deletion is Stage 3)
                match Tmux::kill_session(&session_name) {
                    Ok(_) => {
                        self.message = Some(format!("Killed session '{}' (worktree deletion not yet implemented)", session_name));
                        self.refresh();
                    }
                    Err(e) => self.error = Some(format!("Failed to kill: {}", e)),
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
                    self.message = Some(format!("Renamed '{}' to '{}'", old, new));
                    self.refresh();
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
                    self.message = Some(format!("Created session '{}'", session_name));
                    self.refresh();
                }
                Err(e) => {
                    self.error = Some(format!("Failed to create session: {}", e));
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
            if git.has_upstream {
                // Push: ahead > 0 and clean
                if git.ahead > 0 && !git.is_dirty {
                    actions.push(SessionAction::Push);
                }
                // Pull: behind > 0 and clean
                if git.behind > 0 && !git.is_dirty {
                    actions.push(SessionAction::Pull);
                }
            } else if git.has_remote && !git.is_dirty {
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
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}
