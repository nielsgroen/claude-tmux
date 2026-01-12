# Git & Worktree Awareness Design Document

This document outlines the staged implementation plan for adding git and worktree awareness to claude-tmux.

## Overview

**Goal**: Make sessions git-aware by detecting git metadata from session working directories, enabling contextual actions like showing branch names, dirty state indicators, and offering worktree deletion when killing sessions.

**Key Design Decisions**:
- Use `git2` crate (libgit2 bindings) instead of shelling out to git
- Git context is *enrichment* on existing sessions, not a separate concept
- h/l keys repurposed for sub-menu navigation (actions menu)
- Staged rollout to maintain stability

---

## Stage 1: Git Metadata Detection & Display

**Goal**: Detect basic git info and display it in the session list.

### Data Model

```rust
// src/git.rs (new file)

/// Git context for a session's working directory
pub struct GitContext {
    /// Current branch name (or HEAD if detached)
    pub branch: String,
    /// Whether the working directory has uncommitted changes
    pub is_dirty: bool,
    /// Whether this directory is a worktree (not the main checkout)
    pub is_worktree: bool,
    /// Path to the main repository (if this is a worktree)
    pub main_repo_path: Option<PathBuf>,
}

impl GitContext {
    /// Detect git context for a given path. Returns None if not a git repo.
    pub fn detect(path: &Path) -> Option<Self> { ... }
}
```

```rust
// src/session.rs - extend Session

pub struct Session {
    // ... existing fields ...

    /// Git context, if the working directory is a git repository
    pub git_context: Option<GitContext>,
}
```

### Implementation Details

**Detection using git2**:
```rust
use git2::Repository;

pub fn detect(path: &Path) -> Option<GitContext> {
    let repo = Repository::discover(path).ok()?;

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

    // Check dirty state
    let is_dirty = repo.statuses(None)
        .map(|statuses| !statuses.is_empty())
        .unwrap_or(false);

    // Check if worktree
    let is_worktree = repo.is_worktree();
    let main_repo_path = if is_worktree {
        repo.commondir().map(|p| p.to_path_buf())
    } else {
        None
    };

    Some(GitContext {
        branch,
        is_dirty,
        is_worktree,
        main_repo_path,
    })
}
```

### UI Changes

**Session list format**:
```
● my-feature        Working    ~/repos/project (feature-123) *
○ main-dev          Idle       ~/repos/project (main)
◐ review-pr         Waiting    ~/repos/other [pr-456]        # [] for worktrees
? legacy            Unknown    ~/old-stuff                    # no git info
```

**Legend**:
- `(branch)` - regular git repo with branch name
- `[branch]` - git worktree with branch name
- `*` suffix - dirty working directory
- No annotation - not a git repo

**Changes to `ui.rs`**:
- Modify `render_session_list()` to append git info after path
- Add styling: branch in cyan, `*` in yellow, `[]` brackets in magenta

### Files to Modify/Create

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `git2` dependency |
| `src/git.rs` | **New file** - GitContext struct and detection |
| `src/session.rs` | Add `git_context: Option<GitContext>` field |
| `src/tmux.rs` | Call `GitContext::detect()` when building sessions |
| `src/ui.rs` | Render git info in session list |
| `src/lib.rs` or `main.rs` | Add `mod git;` |

### Performance Considerations

- `git2` is fast (no subprocess overhead)
- `Repository::discover()` walks up to find `.git` - usually instant
- `repo.statuses()` can be slow on large repos with many files
  - Consider: only check dirty state for selected session?
  - Or: use `StatusOptions` to limit scope

---

## Stage 2: Sub-Menu Navigation System

**Goal**: Repurpose h/l for navigating into action sub-menus instead of expand/collapse.

### New Navigation Model

```
Normal Mode (session list)
    │
    ├── j/k/↑/↓    Navigate sessions
    ├── l/→        Enter action menu for selected session
    ├── Enter      Go to session
    └── q/Esc      Quit

Action Menu Mode (for a specific session)
    │
    ├── j/k/↑/↓   Navigate actions
    ├── Enter  Execute selected action
    ├── h/←/Esc    Back to session list
    └── q          Quit entirely
```

### Data Model Changes

```rust
// src/app.rs

pub enum Mode {
    Normal,
    ActionMenu,      // NEW: viewing actions for selected session
    Filter,
    ConfirmAction,   // RENAMED from ConfirmKill - now generic
    NewSession,
    Rename,
    Help,
}

pub struct App {
    // ... existing fields ...

    /// Available actions for the selected session (computed)
    pub available_actions: Vec<SessionAction>,
    /// Currently highlighted action in ActionMenu mode
    pub selected_action: usize,
    /// Action pending confirmation
    pub pending_action: Option<SessionAction>,
}

#[derive(Clone)]
pub enum SessionAction {
    SwitchTo,
    Kill { delete_worktree: bool },
    Rename,
    // Future actions can be added here
}

impl SessionAction {
    pub fn label(&self) -> &str {
        match self {
            Self::SwitchTo => "Switch to session",
            Self::Kill { delete_worktree: false } => "Kill session",
            Self::Kill { delete_worktree: true } => "Kill session + delete worktree",
            Self::Rename => "Rename session",
        }
    }

    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::Kill { .. })
    }
}
```

### Action Menu Logic

```rust
impl App {
    /// Compute available actions for the selected session
    pub fn compute_actions(&mut self) {
        let Some(session) = self.selected_session() else {
            self.available_actions = vec![];
            return;
        };

        let mut actions = vec![
            SessionAction::SwitchTo,
            SessionAction::Rename,
        ];

        // Kill action - with worktree option if applicable
        if let Some(git) = &session.git_context {
            if git.is_worktree {
                actions.push(SessionAction::Kill { delete_worktree: false });
                actions.push(SessionAction::Kill { delete_worktree: true });
            } else {
                actions.push(SessionAction::Kill { delete_worktree: false });
            }
        } else {
            actions.push(SessionAction::Kill { delete_worktree: false });
        }

        self.available_actions = actions;
        self.selected_action = 0;
    }

    pub fn enter_action_menu(&mut self) {
        self.compute_actions();
        self.mode = Mode::ActionMenu;
    }

    pub fn execute_selected_action(&mut self) {
        let action = self.available_actions[self.selected_action].clone();
        if action.requires_confirmation() {
            self.pending_action = Some(action);
            self.mode = Mode::ConfirmAction;
        } else {
            self.perform_action(action);
        }
    }
}
```

### UI Changes

**Action menu rendering** (right side panel or overlay):
```
┌─ Actions: my-feature ─────────────┐
│                                   │
│  > Switch to session              │
│    Rename session                 │
│    Stage changes to git           │ (if dirty)
│    Commit changes                 │ (if changes staged)
│    Push to remote                 │ (if ahead of remote)
│    Pull from remote               │ (if (behind and) clean)
│    Kill session                   │
│    Kill session + delete worktree │
│                                   │
│  [Enter] Select  [h/Esc] Back     │
└───────────────────────────────────┘
```

### Input Changes

```rust
// src/input.rs

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.select_prev(),

        // Enter action menu (CHANGED from expand/collapse)
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
            app.enter_action_menu();
        }

        // ... rest unchanged ...
    }
}

fn handle_action_menu_mode(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        // Navigate actions
        KeyCode::Char('j') | KeyCode::Down => {
            app.selected_action = (app.selected_action + 1) % app.available_actions.len();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.selected_action = app.selected_action.saturating_sub(1);
        }

        // Execute action
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
            app.execute_selected_action();
        }

        // Back to session list
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
            app.mode = Mode::Normal;
        }

        // Quit entirely
        KeyCode::Char('q') => return true,

        _ => {}
    }
    false
}
```

### Files to Modify

| File | Changes |
|------|---------|
| `src/app.rs` | Add `SessionAction` enum, action menu state, methods |
| `src/input.rs` | Add `handle_action_menu_mode()`, change h/l behavior |
| `src/ui.rs` | Add `render_action_menu()` |

### Migration Notes

- **Breaking change**: h/l no longer expand/collapse details
- Details could be shown automatically in action menu, or via a dedicated key (e.g., `d` for details)
- Consider: show session details in action menu header

---

## Stage 3: Worktree Management

**Goal**: Full worktree lifecycle support - create sessions from new/existing worktrees, and delete worktrees when killing sessions.

### 3.1 Create Session from Worktree

**Entry point**: Action menu on any session that's in a git repository.

**Action**: "New session from worktree"

#### UX Flow

```
┌─ New Session from Worktree ───────────────────┐
│                                               │
│  Branch:  [feature/new-thing____]  (new)      │
│           ─────────────────────────           │
│           > main                              │
│             develop                           │
│             feature/existing                  │
│           ─────────────────────────           │
│                                               │
│  Path:    [~/repos/project-new-thing__]       │
│                                               │
│  Session: [project-new-thing_________]        │
│                                               │
│  [Tab] Next  [Enter] Create  [Esc] Cancel     │
└───────────────────────────────────────────────┘
```

**Behavior**:
- **Branch field**: Text input that filters/searches existing branches. If input doesn't match an existing branch exactly, it's treated as a new branch name.
- **Path field**: Auto-populated as `{parent_of_repo}/{branch_name}` (sanitized). User can edit.
- **Session field**: Auto-populated from branch name (sanitized). User can edit.

#### Data Model

```rust
// src/app.rs

pub enum Mode {
    // ... existing modes ...

    /// Creating a new session from a worktree
    NewWorktree {
        /// The source repository path (from selected session)
        source_repo: PathBuf,
        /// Branch name input (may be new or existing)
        branch_input: String,
        /// Filtered list of existing branches
        filtered_branches: Vec<String>,
        /// Selected index in filtered branches (None = creating new branch)
        selected_branch: Option<usize>,
        /// Worktree path
        worktree_path: String,
        /// Session name
        session_name: String,
        /// Which field is active
        field: NewWorktreeField,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWorktreeField {
    Branch,
    Path,
    SessionName,
}
```

```rust
// src/app.rs - new action

pub enum SessionAction {
    // ... existing actions ...

    /// Create a new session from a worktree (new or existing branch)
    NewWorktree,
}
```

#### Git Module Extensions

```rust
// src/git.rs additions

impl GitContext {
    /// List all local branch names in the repository
    pub fn list_branches(repo_path: &Path) -> Result<Vec<String>> {
        let repo = Repository::discover(repo_path)?;
        let mut branches = Vec::new();

        for branch in repo.branches(Some(BranchType::Local))? {
            let (branch, _) = branch?;
            if let Some(name) = branch.name()? {
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

    /// Create a new worktree
    /// - If branch exists: checkout that branch in the new worktree
    /// - If branch is new: create branch from HEAD and checkout in worktree
    pub fn create_worktree(
        repo_path: &Path,
        worktree_path: &Path,
        branch_name: &str,
        is_new_branch: bool,
    ) -> Result<()> {
        let repo = Repository::discover(repo_path)?;

        if is_new_branch {
            // Create new branch from HEAD, then create worktree
            let head = repo.head()?;
            let commit = head.peel_to_commit()?;

            // Create the worktree with a new branch
            repo.worktree(
                branch_name,  // worktree name (usually same as branch)
                worktree_path,
                Some(WorktreeAddOptions::new().reference(Some(&commit.id().to_string()))),
            )?;
        } else {
            // Branch exists - create worktree for existing branch
            let branch = repo.find_branch(branch_name, BranchType::Local)?;
            let reference = branch.into_reference();

            repo.worktree(
                branch_name,
                worktree_path,
                Some(WorktreeAddOptions::new().reference(reference.name())),
            )?;
        }

        Ok(())
    }
}
```

#### Input Handling

```rust
// src/input.rs

fn handle_new_worktree_mode(app: &mut App, key: KeyEvent) -> bool {
    if let Mode::NewWorktree {
        ref mut branch_input,
        ref mut filtered_branches,
        ref mut selected_branch,
        ref mut worktree_path,
        ref mut session_name,
        ref mut field,
        ref source_repo,
    } = app.mode {
        match key.code {
            KeyCode::Tab => {
                // Cycle through fields
                *field = match field {
                    NewWorktreeField::Branch => NewWorktreeField::Path,
                    NewWorktreeField::Path => NewWorktreeField::SessionName,
                    NewWorktreeField::SessionName => NewWorktreeField::Branch,
                };
            }
            KeyCode::Enter => {
                app.confirm_new_worktree();
            }
            KeyCode::Esc => {
                app.mode = Mode::Normal;
            }
            KeyCode::Char(c) => {
                match field {
                    NewWorktreeField::Branch => {
                        branch_input.push(c);
                        // Re-filter branches and update suggestions
                        app.update_worktree_suggestions();
                    }
                    NewWorktreeField::Path => worktree_path.push(c),
                    NewWorktreeField::SessionName => session_name.push(c),
                }
            }
            KeyCode::Backspace => {
                match field {
                    NewWorktreeField::Branch => {
                        branch_input.pop();
                        app.update_worktree_suggestions();
                    }
                    NewWorktreeField::Path => { worktree_path.pop(); }
                    NewWorktreeField::SessionName => { session_name.pop(); }
                }
            }
            // Navigate branch suggestions when in Branch field
            KeyCode::Down if *field == NewWorktreeField::Branch => {
                if !filtered_branches.is_empty() {
                    *selected_branch = Some(
                        selected_branch.map(|i| (i + 1) % filtered_branches.len()).unwrap_or(0)
                    );
                }
            }
            KeyCode::Up if *field == NewWorktreeField::Branch => {
                if !filtered_branches.is_empty() {
                    *selected_branch = Some(
                        selected_branch
                            .map(|i| if i == 0 { filtered_branches.len() - 1 } else { i - 1 })
                            .unwrap_or(filtered_branches.len() - 1)
                    );
                }
            }
            _ => {}
        }
    }
    false
}
```

### 3.2 Delete Worktree

**Entry point**: Action menu on a session that's in a worktree (not main repo).

**Action**: "Kill session + delete worktree"

#### Git Module Extensions

```rust
// src/git.rs additions

impl GitContext {
    /// Delete the worktree at the given path
    /// Returns an error if the worktree has uncommitted changes (unless force=true)
    pub fn delete_worktree(worktree_path: &Path, force: bool) -> Result<()> {
        let repo = Repository::discover(worktree_path)?;

        // We need to open the main repo to manage worktrees
        let main_repo = if repo.is_worktree() {
            Repository::open(repo.commondir())?
        } else {
            return Err(anyhow!("Not a worktree"));
        };

        // Find the worktree by path
        let worktrees = main_repo.worktrees()?;
        for name in worktrees.iter() {
            let name = name.ok_or_else(|| anyhow!("Invalid worktree name"))?;
            let wt = main_repo.find_worktree(name)?;

            if wt.path() == worktree_path {
                // Validate it's safe to delete (checks for uncommitted changes)
                if !force {
                    wt.validate()
                        .context("Worktree has uncommitted changes or other issues")?;
                }

                // Prune the worktree from git's tracking
                let mut prune_opts = git2::WorktreePruneOptions::new();
                if force {
                    prune_opts.valid(true);  // Prune even if valid
                }
                wt.prune(Some(&mut prune_opts))?;

                // Remove the directory from disk
                std::fs::remove_dir_all(worktree_path)
                    .context("Failed to remove worktree directory")?;

                return Ok(());
            }
        }

        Err(anyhow!("Worktree not found"))
    }
}
```

#### Safety Checks

Before deleting a worktree, verify:
1. It actually is a worktree (not main repo)
2. Working directory is clean (refuse if dirty - no force option in UI)

```rust
impl App {
    fn can_delete_worktree(&self, session: &Session) -> Result<(), &'static str> {
        let git = session.git_context.as_ref()
            .ok_or("Not a git repository")?;

        if !git.is_worktree {
            return Err("Not a worktree");
        }

        if git.is_dirty() {
            return Err("Worktree has uncommitted changes");
        }

        Ok(())
    }
}
```

#### Confirmation Dialog Enhancement

```
┌─ Confirm Action ──────────────────────────────┐
│                                               │
│  Kill session "my-feature"                    │
│  AND delete worktree at ~/repos/project-wt    │
│                                               │
│  ⚠ This will permanently delete the           │
│    worktree directory                         │
│                                               │
│  [y] Confirm    [n] Cancel                    │
└───────────────────────────────────────────────┘
```

### 3.3 Helper Functions

```rust
// src/app.rs or src/util.rs

/// Sanitize a branch name for use as a directory/session name
/// e.g., "feature/new-thing" -> "new-thing"
fn sanitize_for_path(branch: &str) -> String {
    branch
        .rsplit('/')
        .next()
        .unwrap_or(branch)
        .replace(['/', '\\', ' ', ':'], "-")
}

/// Generate default worktree path from repo path and branch name
/// e.g., ~/repos/project + feature/foo -> ~/repos/project-foo
fn default_worktree_path(repo_path: &Path, branch: &str) -> PathBuf {
    let parent = repo_path.parent().unwrap_or(repo_path);
    let repo_name = repo_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo");
    let branch_suffix = sanitize_for_path(branch);
    parent.join(format!("{}-{}", repo_name, branch_suffix))
}
```

### Files to Modify

| File | Changes |
|------|---------|
| `src/git.rs` | Add `list_branches()`, `create_worktree()`, `delete_worktree()` |
| `src/app.rs` | Add `Mode::NewWorktree`, `SessionAction::NewWorktree`, worktree creation/deletion logic, helper functions |
| `src/input.rs` | Add `handle_new_worktree_mode()` |
| `src/ui.rs` | Add `render_new_worktree_dialog()`, enhance confirmation dialog |

### Implementation Order

| Step | Task | Depends On |
|------|------|------------|
| 3.1a | `git.rs`: Add `list_branches()` | - |
| 3.1b | `git.rs`: Add `create_worktree()` | - |
| 3.1c | `app.rs`: Add `Mode::NewWorktree` and `SessionAction::NewWorktree` | 3.1a |
| 3.1d | `input.rs`: Add `handle_new_worktree_mode()` | 3.1c |
| 3.1e | `ui.rs`: Add `render_new_worktree_dialog()` | 3.1c |
| 3.1f | `app.rs`: Wire up `confirm_new_worktree()` | 3.1b, 3.1d |
| 3.2a | `git.rs`: Add `delete_worktree()` | - |
| 3.2b | `app.rs`: Add `can_delete_worktree()` | 3.2a |
| 3.2c | `app.rs`: Implement `KillAndDeleteWorktree` action | 3.2a, 3.2b |
| 3.2d | `ui.rs`: Enhance confirmation dialog | 3.2c |

---

## Stage 4: Polish & Edge Cases

**Goal**: Handle edge cases, improve UX, add tests.

### Edge Cases to Handle

1. **Detached HEAD**: Show short commit hash instead of branch name
2. **Empty repository**: Handle repos with no commits
3. **Bare repository**: Skip git detection for bare repos
4. **Nested worktrees**: Unlikely but possible
5. **Permission errors**: Handle gracefully if `.git` isn't readable
6. **Submodules**: Should show submodule's branch, not parent's

### Testing

```rust
// src/git.rs tests

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use git2::Repository;

    #[test]
    fn test_detect_regular_repo() {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        // Create initial commit...

        let ctx = GitContext::detect(dir.path()).unwrap();
        assert_eq!(ctx.branch, "main"); // or "master" depending on git config
        assert!(!ctx.is_worktree);
        assert!(!ctx.is_dirty);
    }

    #[test]
    fn test_detect_worktree() {
        // Create main repo
        // Create worktree
        // Verify detection
    }

    #[test]
    fn test_detect_dirty_state() {
        // Create repo, modify file, check is_dirty
    }

    #[test]
    fn test_non_git_directory() {
        let dir = TempDir::new().unwrap();
        assert!(GitContext::detect(dir.path()).is_none());
    }
}
```

### Documentation Updates

- Update README with new keybindings (h/l for action menu)
- Document git-aware features
- Add screenshots showing git info in session list

---

## Implementation Order

| Stage | Deliverable | Depends On |
|-------|-------------|------------|
| 1a | `git.rs` with `GitContext::detect()` | - |
| 1b | Session struct extended with git_context | 1a |
| 1c | UI shows git info (branch, dirty, worktree indicator) | 1b |
| 2a | `SessionAction` enum and action menu state | - |
| 2b | Action menu UI rendering | 2a |
| 2c | h/l navigation changes | 2a, 2b |
| 3a | `delete_worktree()` implementation | 1a |
| 3b | Safety checks before deletion | 1b, 3a |
| 3c | Enhanced confirmation dialog | 2b, 3b |
| 4 | Edge cases, tests, docs | All above |

---

## Open Questions

1. **Dirty state performance**: Should we check dirty state eagerly for all sessions, or lazily only for the selected session? Eagerly.

2. **Details panel**: With h/l repurposed, how should users see expanded session details? No.

3. **Quick actions**: Should some actions (like switch-to) remain accessible directly from normal mode (Enter), or always go through action menu? Enter in normal mode switches immediately.

4. **Future actions**: What other actions might benefit from the action menu?
   - Open in editor
   - Copy path to clipboard
   - Create new session from same worktree
   - Git operations (fetch, pull)?

---

## Dependencies

```toml
# Cargo.toml additions
[dependencies]
git2 = "0.20"  # or latest stable
```
