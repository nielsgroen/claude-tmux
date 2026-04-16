use crate::session::ClaudeCodeStatus;

/// Detect Claude Code status from pane content.
pub fn detect_status(content: &str) -> ClaudeCodeStatus {
    // Step 1: Detect input field by its visual structure
    if has_input_field(content) {
        // Step 2: Check if interruptable
        if content.contains("ctrl+c") && content.contains("to interrupt") {
            return ClaudeCodeStatus::Working;
        }
        return ClaudeCodeStatus::Idle;
    }

    // No input field - check for permission prompt
    if content.contains("[y/n]") || content.contains("[Y/n]") {
        return ClaudeCodeStatus::WaitingInput;
    }

    ClaudeCodeStatus::Unknown
}

/// Returns true if the content looks like an active Claude Code pane.
///
/// Used as a fallback when the pane's process name does not match "claude"
/// (e.g. Claude Code sets its process title to its version number such as
/// "2.1.110", which defeats command-name based detection).
pub fn looks_like_claude_pane(content: &str) -> bool {
    has_input_field(content)
}

/// Detect input field: prompt line (❯) with border directly above it.
fn has_input_field(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if line.contains('❯') {
            // Check if line above is a border
            if i > 0 && lines[i - 1].contains('─') {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_working() {
        // Border directly above prompt
        let content = "* (ctrl+c to interrupt)\n─────\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_idle() {
        // Border directly above prompt
        let content = "● Done\n─────\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Idle);
    }

    #[test]
    fn test_no_border_above_prompt() {
        // Border exists but not directly above prompt - should be unknown
        let content = "─────\nsome text\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Unknown);
    }

    #[test]
    fn test_waiting_input() {
        let content = "Delete files? [y/n]";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_unknown() {
        let content = "random stuff";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Unknown);
    }

    #[test]
    fn test_looks_like_claude_pane() {
        // Matches a pane with Claude Code's distinctive UI
        let content = "● Done\n─────\n❯ hello";
        assert!(looks_like_claude_pane(content));
    }

    #[test]
    fn test_looks_like_claude_pane_with_status_lines() {
        // Status/mode lines below the input box do not affect identification
        let content = "● Done\n─────\n❯ hello\n─────\n  ~/repo | Sonnet 4.6\n  ►► auto mode on";
        assert!(looks_like_claude_pane(content));
    }

    #[test]
    fn test_not_claude_pane() {
        let content = "regular terminal output";
        assert!(!looks_like_claude_pane(content));
    }
}
