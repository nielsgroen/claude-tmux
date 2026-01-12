# CCM: Claude Code Session Manager

## Design Document

**Version:** 0.1  
**Date:** January 2026  
**Status:** Draft

---

## Overview

CCM (Claude Code Manager) is a terminal user interface application for managing multiple Claude Code sessions within tmux. It provides a centralized view of all Claude Code instances, enabling quick switching, status monitoring, and session lifecycle management.

The application is designed to be invoked via tmux's `display-popup` command, appearing as an overlay that dismisses after an action is taken.

---

## Problem Statement

When working on multiple projects simultaneously, developers often run several Claude Code instances in separate tmux sessions. Managing these becomes cumbersome:

- Difficult to remember which sessions exist and what they're working on
- No unified view of Claude Code activity across sessions
- Context switching requires manual `tmux switch-client` commands
- No visibility into whether a Claude Code instance is idle, working, or waiting for input

---

## Goals

1. **Quick Overview** — See all Claude Code sessions at a glance
2. **Fast Switching** — Switch to any session with minimal keystrokes
3. **Status Visibility** — Know what each Claude Code instance is doing
4. **Session Management** — Create, kill, and organize sessions
5. **Minimal Footprint** — Popup interface that stays out of the way

---

## Non-Goals

- Replacing tmux or providing general tmux management
- Interacting with Claude Code's internals or API
- Managing non-Claude-Code tmux sessions (though they may be visible)
- Persistent background daemon

---

## Core Concepts

### Session Model

CCM enforces a **one Claude Code instance per tmux session** model:

- Each tmux session contains at most one Claude Code process
- The CC instance typically runs in pane 0, with other panes for editors, shells, tests, etc.
- Sessions are the unit of project context — switching sessions means switching projects
- This constraint simplifies management and provides natural isolation between projects

Example session layout:
```
Session: my-project
├── Pane 0: Claude Code ← managed by CCM
├── Pane 1: nvim
├── Pane 2: shell
└── Pane 3: test runner
```

---

## Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────────┐
│                    tmux display-popup                    │
│  ┌───────────────────────────────────────────────────┐  │
│  │                      CCM TUI                       │  │
│  │  ┌─────────────────────────────────────────────┐  │  │
│  │  │  Sessions List                              │  │  │
│  │  │  ─────────────────────────────────────────  │  │  │
│  │  │  > project-alpha    [working]    ~/dev/alpha│  │  │
│  │  │    project-beta     [idle]       ~/dev/beta │  │  │
│  │  │    dotfiles         [input]      ~/.dotfiles│  │  │
│  │  └─────────────────────────────────────────────┘  │  │
│  │                                                    │  │
│  │  [n]ew  [k]ill  [r]ename  [enter]switch  [q]uit   │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Components

#### 1. Session Discovery

Queries tmux to find all sessions and identify which contain Claude Code:

```rust
struct Session {
    name: String,
    created: DateTime,
    attached: bool,
    working_directory: PathBuf,
    panes: Vec<Pane>,
    claude_code_pane: Option<PaneId>,
    claude_code_status: Option<ClaudeCodeStatus>,
}

enum ClaudeCodeStatus {
    Idle,           // Waiting at prompt
    Working,        // Actively processing
    WaitingInput,   // Awaiting user confirmation/input
    Unknown,        // Can't determine
}
```

Discovery process:
1. Run `tmux list-sessions -F "#{session_name}:#{session_created}:#{session_attached}"`
2. For each session, run `tmux list-panes -t <session> -F "#{pane_id}:#{pane_current_command}:#{pane_current_path}"`
3. Identify panes running `claude` process
4. Optionally capture last few lines of pane to infer status

#### 2. Status Detection (Best Effort)

Detecting Claude Code status is approximate since we're external to the process:

| Signal | Inferred Status |
|--------|-----------------|
| `claude` process running, recent output contains spinner/progress | Working |
| `claude` process running, prompt visible (`❯` or similar) | Idle |
| `claude` process running, confirmation prompt visible (`y/n`) | WaitingInput |
| No `claude` process in session | Not a CC session |

Implementation:
```rust
fn detect_status(pane_id: &str) -> ClaudeCodeStatus {
    // Capture last N lines of pane
    let output = tmux_capture_pane(pane_id, lines: 10);
    
    // Heuristic matching
    if output.contains("[y/n]") || output.contains("Continue?") {
        ClaudeCodeStatus::WaitingInput
    } else if output.contains("⠋") || output.contains("⠙") /* spinner chars */ {
        ClaudeCodeStatus::Working
    } else if output.ends_with("❯ ") || output.contains("claude>") {
        ClaudeCodeStatus::Idle
    } else {
        ClaudeCodeStatus::Unknown
    }
}
```

#### 3. TUI Layer

Built with Ratatui, optimized for popup use:

**Layout:**
- Header: "CCM - Claude Code Manager" + current session indicator
- Body: Scrollable session list with status indicators
- Footer: Keybinding hints

**Keybindings:**

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `Enter` | Switch to selected session (closes popup) |
| `/` | Fuzzy search/filter sessions |
| `n` | New session (prompts for name and path) |
| `K` | Kill selected session (with confirmation) |
| `r` | Rename selected session |
| `R` | Refresh session list |
| `q` / `Esc` | Quit without action |
| `?` | Show help |

**Visual Design:**
```
╭─ CCM ─────────────────────────── attached: project-alpha ─╮
│                                                           │
│  NAME              STATUS      PATH                       │
│  ────────────────────────────────────────────────────     │
│ ▸ project-alpha    ● working   ~/dev/alpha                │
│   project-beta     ○ idle      ~/dev/beta                 │
│   dotfiles         ◐ input     ~/.dotfiles                │
│   experiments      ○ idle      ~/lab                      │
│                                                           │
│  4 sessions │ 1 working │ 1 awaiting input                │
├───────────────────────────────────────────────────────────┤
│  ↑↓ navigate  ⏎ switch  n new  K kill  / filter  ? help   │
╰───────────────────────────────────────────────────────────╯
```

Status indicators:
- `●` (filled, green): Working
- `○` (empty, dim): Idle  
- `◐` (half, yellow): Waiting for input
- `?` (gray): Unknown/not CC session

#### 4. tmux Integration

**Invocation:**

Add to `.tmux.conf`:
```bash
bind-key C-c display-popup -E -w 80 -h 20 "ccm"
```

Options:
- `-E`: Close popup when command exits
- `-w 80 -h 20`: Popup dimensions (adjust as needed)

**Actions executed by CCM:**

```rust
fn switch_to_session(session: &str) {
    // Switches the current client to target session
    Command::new("tmux")
        .args(["switch-client", "-t", session])
        .status();
}

fn new_session(name: &str, path: &Path) {
    Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-c", path.to_str().unwrap()])
        .status();
    // Optionally start Claude Code in the new session
    Command::new("tmux")
        .args(["send-keys", "-t", name, "claude", "Enter"])
        .status();
}

fn kill_session(session: &str) {
    Command::new("tmux")
        .args(["kill-session", "-t", session])
        .status();
}
```

---

## Data Flow

```
┌──────────┐     list-sessions      ┌───────────┐
│          │ ───────────────────▸   │           │
│   tmux   │                        │    CCM    │
│          │ ◂───────────────────   │           │
└──────────┘     session data       └─────┬─────┘
     ▲                                    │
     │         switch-client              │
     └────────────────────────────────────┘
```

1. CCM launches, queries tmux for session data
2. User navigates list, CCM shows current state
3. User presses Enter, CCM executes `tmux switch-client`
4. CCM exits, popup closes, user is in new session

---

## Project Structure

```
ccm/
├── Cargo.toml
├── src/
│   ├── main.rs           # Entry point, arg parsing
│   ├── app.rs            # Application state machine
│   ├── ui.rs             # Ratatui rendering
│   ├── tmux.rs           # tmux interaction layer
│   ├── session.rs        # Session data structures
│   ├── detection.rs      # Claude Code status detection
│   └── input.rs          # Keyboard handling
└── README.md
```

---

## Dependencies

```toml
[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["rt", "process"] }
anyhow = "1.0"
dirs = "5.0"          # For path expansion
unicode-width = "0.2" # For proper text alignment
```

---

## Future Enhancements

### Phase 2
- **Preview pane**: Show last N lines of selected session's Claude Code output
- **Fuzzy finder**: Filter sessions by name with fuzzy matching
- **Session groups**: Organize sessions by project/tag
- **Auto-refresh**: Periodically update status without manual refresh

### Phase 3
- **Claude Code integration**: If CC exposes any IPC/status file, read it directly
- **Persistent state**: Remember window arrangements per session
- **Multi-attach**: View multiple CC outputs in split view

---

## Open Questions

1. **Status detection reliability**: How accurate can we be with pane content heuristics? Should we propose a status file to Claude Code team?

2. **Non-CC sessions**: Should we show all tmux sessions (dimmed) or only those with Claude Code?

3. **Session naming convention**: Should CCM enforce/suggest naming patterns (e.g., `cc-<project>`)?

4. **Startup command**: When creating a new session, should CCM auto-start `claude` or leave that to the user?

---

## References

- [Ratatui documentation](https://ratatui.rs/)
- [tmux man page](https://man7.org/linux/man-pages/man1/tmux.1.html)
- [Claude Code documentation](https://docs.anthropic.com/en/docs/claude-code)

