use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use crate::detection::detect_status;
use crate::session::{Pane, Session};

/// Wrapper for tmux command execution
pub struct Tmux;

impl Tmux {
    /// List all tmux sessions with their metadata
    pub fn list_sessions() -> Result<Vec<Session>> {
        let output = Command::new("tmux")
            .args([
                "list-sessions",
                "-F",
                "#{session_name}\t#{session_created}\t#{session_attached}",
            ])
            .output()
            .context("Failed to execute tmux list-sessions")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // No sessions is not an error for us
            if stderr.contains("no server running") || stderr.contains("no sessions") {
                return Ok(Vec::new());
            }
            anyhow::bail!("tmux list-sessions failed: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut sessions = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let name = parts[0].to_string();
                let created = parts[1].parse().unwrap_or(0);
                let attached = parts[2] == "1";

                // Get panes for this session
                let panes = Self::list_panes(&name).unwrap_or_default();

                // Find Claude Code pane and detect status
                let (claude_code_pane, claude_code_status, working_directory) =
                    Self::find_claude_code_pane(&name, &panes);

                // Use the Claude Code pane's path, or fall back to first pane's path
                let working_directory = working_directory.unwrap_or_else(|| {
                    panes
                        .first()
                        .map(|p| p.current_path.clone())
                        .unwrap_or_default()
                });

                sessions.push(Session {
                    name,
                    created,
                    attached,
                    working_directory,
                    panes,
                    claude_code_pane,
                    claude_code_status,
                });
            }
        }

        // Sort by attached status first, then by name
        sessions.sort_by(|a, b| {
            b.attached
                .cmp(&a.attached)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(sessions)
    }

    /// List all panes in a session
    fn list_panes(session: &str) -> Result<Vec<Pane>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-t",
                session,
                "-F",
                "#{pane_id}\t#{pane_current_command}\t#{pane_current_path}",
            ])
            .output()
            .context("Failed to execute tmux list-panes")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut panes = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                panes.push(Pane {
                    id: parts[0].to_string(),
                    current_command: parts[1].to_string(),
                    current_path: PathBuf::from(parts[2]),
                });
            }
        }

        Ok(panes)
    }

    /// Find the pane running Claude Code and detect its status
    fn find_claude_code_pane(
        _session: &str,
        panes: &[Pane],
    ) -> (
        Option<String>,
        crate::session::ClaudeCodeStatus,
        Option<PathBuf>,
    ) {
        use crate::session::ClaudeCodeStatus;

        for pane in panes {
            // Check if this pane is running claude
            if pane.current_command == "claude" || pane.current_command.contains("claude") {
                // Capture pane content to detect status (strip empty lines for detection)
                let status = Self::capture_pane(&pane.id, 15, true)
                    .map(|content| detect_status(&content))
                    .unwrap_or(ClaudeCodeStatus::Unknown);

                return (
                    Some(pane.id.clone()),
                    status,
                    Some(pane.current_path.clone()),
                );
            }
        }

        (None, ClaudeCodeStatus::Unknown, None)
    }

    /// Capture the last N lines of a pane's content
    ///
    /// If `strip_empty` is true, empty lines are filtered out before taking the last N.
    /// This is useful for status detection. For preview display, use `strip_empty: false`
    /// to preserve the visual layout.
    ///
    /// ANSI escape sequences are always included - the UI handles rendering them.
    pub fn capture_pane(pane_id: &str, lines: usize, strip_empty: bool) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                pane_id,
                "-p", // Print to stdout
                "-J", // Join wrapped lines
                "-e", // Include escape sequences
            ])
            .output()
            .context("Failed to capture pane")?;

        if !output.status.success() {
            anyhow::bail!("Failed to capture pane {}", pane_id);
        }

        let content = String::from_utf8_lossy(&output.stdout);

        if strip_empty {
            // Filter out empty lines, then get last N (for status detection)
            let non_empty: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
            let start = non_empty.len().saturating_sub(lines);
            let last_lines = &non_empty[start..];
            Ok(last_lines.join("\n"))
        } else {
            // Preserve internal empty lines but trim trailing ones (for preview display)
            let all_lines: Vec<&str> = content.lines().collect();

            // Find last non-empty line
            let last_non_empty = all_lines
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map(|i| i + 1)
                .unwrap_or(0);

            let trimmed = &all_lines[..last_non_empty];
            let start = trimmed.len().saturating_sub(lines);
            let last_lines = &trimmed[start..];
            Ok(last_lines.join("\n"))
        }
    }

    /// Switch the current client to the specified session
    pub fn switch_to_session(session: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["switch-client", "-t", session])
            .status()
            .context("Failed to switch session")?;

        if !status.success() {
            anyhow::bail!("Failed to switch to session {}", session);
        }

        Ok(())
    }

    /// Create a new tmux session
    pub fn new_session(name: &str, path: &std::path::Path, start_claude: bool) -> Result<()> {
        let path_str = path.to_string_lossy();

        let status = Command::new("tmux")
            .args(["new-session", "-d", "-s", name, "-c", &path_str])
            .status()
            .context("Failed to create new session")?;

        if !status.success() {
            anyhow::bail!("Failed to create session {}", name);
        }

        if start_claude {
            // Send claude command to the new session
            let _ = Command::new("tmux")
                .args(["send-keys", "-t", name, "claude", "Enter"])
                .status();
        }

        Ok(())
    }

    /// Kill a tmux session
    pub fn kill_session(session: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["kill-session", "-t", session])
            .status()
            .context("Failed to kill session")?;

        if !status.success() {
            anyhow::bail!("Failed to kill session {}", session);
        }

        Ok(())
    }

    /// Rename a tmux session
    pub fn rename_session(old_name: &str, new_name: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["rename-session", "-t", old_name, new_name])
            .status()
            .context("Failed to rename session")?;

        if !status.success() {
            anyhow::bail!("Failed to rename session {} to {}", old_name, new_name);
        }

        Ok(())
    }

    /// Get the name of the currently attached session
    pub fn current_session() -> Result<Option<String>> {
        let output = Command::new("tmux")
            .args(["display-message", "-p", "#{session_name}"])
            .output()
            .context("Failed to get current session")?;

        if !output.status.success() {
            return Ok(None);
        }

        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() {
            Ok(None)
        } else {
            Ok(Some(name))
        }
    }
}
