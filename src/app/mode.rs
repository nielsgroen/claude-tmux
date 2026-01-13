//! Application mode and action types
//!
//! Defines the various states/modes the application can be in,
//! and the actions that can be performed on sessions.

use std::path::PathBuf;

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
    NewSession {
        name: String,
        path: String,
        field: NewSessionField,
        /// Path completion suggestions
        path_suggestions: Vec<String>,
        /// Currently selected path suggestion index
        path_selected: Option<usize>,
    },
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
        /// Path completion suggestions
        path_suggestions: Vec<String>,
        /// Currently selected path suggestion index
        path_selected: Option<usize>,
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
    /// Fetch from remote (update tracking branches)
    Fetch,
    /// Pull commits from remote
    Pull,
    /// Create a pull request
    CreatePullRequest,
    /// View pull request in browser
    ViewPullRequest,
    /// Close pull request without merging
    ClosePullRequest,
    /// Merge pull request
    MergePullRequest,
    /// Merge PR, delete branch, remove worktree, kill session
    MergePullRequestAndClose,
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
            Self::Fetch => "Fetch from remote",
            Self::Pull => "Pull from remote",
            Self::CreatePullRequest => "Create pull request",
            Self::ViewPullRequest => "View pull request",
            Self::ClosePullRequest => "Close pull request",
            Self::MergePullRequest => "Merge pull request",
            Self::MergePullRequestAndClose => "Merge PR + close session",
            Self::Kill => "Kill session",
            Self::KillAndDeleteWorktree => "Kill session + delete worktree",
        }
    }

    /// Whether this action requires confirmation
    pub fn requires_confirmation(&self) -> bool {
        matches!(
            self,
            Self::Kill
                | Self::KillAndDeleteWorktree
                | Self::ClosePullRequest
                | Self::MergePullRequest
                | Self::MergePullRequestAndClose
        )
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
