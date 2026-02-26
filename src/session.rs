use std::path::PathBuf;

use crate::git::GitContext;

/// Status of a Claude Code instance in a pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaudeCodeStatus {
    /// Waiting at prompt, ready for input
    Idle,
    /// Actively processing a request
    Working,
    /// Awaiting user confirmation/input (y/n prompt, etc.)
    WaitingInput,
    /// Cannot determine status
    #[default]
    Unknown,
}

impl ClaudeCodeStatus {
    /// Returns the display symbol for this status
    pub fn symbol(&self) -> &'static str {
        match self {
            ClaudeCodeStatus::Idle => "○",
            ClaudeCodeStatus::Working => "●",
            ClaudeCodeStatus::WaitingInput => "◐",
            ClaudeCodeStatus::Unknown => "?",
        }
    }

    /// Returns the display label for this status
    pub fn label(&self) -> &'static str {
        match self {
            ClaudeCodeStatus::Idle => "idle",
            ClaudeCodeStatus::Working => "working",
            ClaudeCodeStatus::WaitingInput => "input",
            ClaudeCodeStatus::Unknown => "unknown",
        }
    }
}

/// A tmux pane within a session
#[derive(Debug, Clone)]
pub struct Pane {
    /// Pane ID (e.g., "%0")
    pub id: String,
    /// Current command running in the pane
    pub current_command: String,
    /// Name of the window containing this pane
    pub window_name: String,
    /// Current working directory
    pub current_path: PathBuf,
}

/// A tmux session that may contain a Claude Code instance
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Session {
    /// Session name
    pub name: String,
    /// Unix timestamp when session was created
    pub created: i64,
    /// Whether a client is attached to this session
    pub attached: bool,
    /// Working directory (from the Claude Code pane, or first pane)
    pub working_directory: PathBuf,
    /// Number of windows in this session
    pub window_count: usize,
    /// All panes in this session
    pub panes: Vec<Pane>,
    /// Pane ID containing Claude Code, if any
    pub claude_code_pane: Option<String>,
    /// Status of Claude Code in this session
    pub claude_code_status: ClaudeCodeStatus,
    /// Git context, if the working directory is a git repository
    pub git_context: Option<GitContext>,
}

impl Session {
    /// Returns a shortened version of the working directory for display
    pub fn display_path(&self) -> String {
        let path = &self.working_directory;

        // Try to replace home directory with ~
        if let Some(home) = dirs::home_dir() {
            if let Ok(stripped) = path.strip_prefix(&home) {
                return format!("~/{}", stripped.display());
            }
        }

        path.display().to_string()
    }

    /// Returns a human-readable duration since session creation
    pub fn duration(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let elapsed_secs = (now - self.created).max(0) as u64;

        let days = elapsed_secs / 86400;
        let hours = (elapsed_secs % 86400) / 3600;
        let minutes = (elapsed_secs % 3600) / 60;

        if days > 0 {
            format!("{}d {}h", days, hours)
        } else if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}m", minutes.max(1))
        }
    }
}
