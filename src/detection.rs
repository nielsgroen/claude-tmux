use crate::session::ClaudeCodeStatus;

/// Detect Claude Code status when content has NOT changed since the last check.
///
/// Working is determined externally by content-change detection. This function
/// only distinguishes Idle, WaitingInput, and Unknown from static content.
pub fn detect_static_status(content: &str) -> ClaudeCodeStatus {
    if content.contains("[y/n]") || content.contains("[Y/n]") {
        return ClaudeCodeStatus::WaitingInput;
    }
    if has_input_field(content) {
        return ClaudeCodeStatus::Idle;
    }
    ClaudeCodeStatus::Unknown
}

/// Detect Claude Code status from pane content.
///
/// Used as a fallback when no previous capture is available for comparison.
/// Prefer content-change detection (see `App::tick_status`) for reliable
/// Working vs Idle discrimination.
pub fn detect_status(content: &str) -> ClaudeCodeStatus {
    if has_input_field(content) {
        if content.contains("ctrl+c") && content.contains("to interrupt") {
            return ClaudeCodeStatus::Working;
        }
        return ClaudeCodeStatus::Idle;
    }

    if content.contains("ctrl+c") && content.contains("to interrupt") {
        return ClaudeCodeStatus::Working;
    }

    if content.contains("[y/n]") || content.contains("[Y/n]") {
        return ClaudeCodeStatus::WaitingInput;
    }

    ClaudeCodeStatus::Unknown
}

/// Heuristically decide whether a pane is running Claude Code based on its
/// content, independent of `pane_current_command`.
///
/// Recent Claude Code versions set their process title to the version string
/// (e.g. "2.1.169"), so tmux's `pane_current_command` no longer contains
/// "claude". Matching by name therefore misses every modern session. We fall
/// back to recognizing the Claude UI: the input field (a `❯` prompt with a
/// border directly above), the "ctrl+c to interrupt" working message, or a
/// `[y/n]` permission prompt.
pub fn looks_like_claude(content: &str) -> bool {
    has_input_field(content)
        || (content.contains("ctrl+c") && content.contains("to interrupt"))
        || content.contains("[y/n]")
        || content.contains("[Y/n]")
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
    fn test_looks_like_claude_input_field() {
        // Idle Claude UI: border directly above the prompt.
        let content = "※ recap: did a thing\n─────\n❯ \n─────";
        assert!(looks_like_claude(content));
    }

    #[test]
    fn test_looks_like_claude_working() {
        let content = "* Brewing… (ctrl+c to interrupt)";
        assert!(looks_like_claude(content));
    }

    #[test]
    fn test_looks_like_claude_permission_prompt() {
        let content = "Delete files? [y/n]";
        assert!(looks_like_claude(content));
    }

    #[test]
    fn test_looks_like_claude_rejects_plain_shell() {
        let content = "josec@host ~ % ls\nfoo bar baz";
        assert!(!looks_like_claude(content));
    }
}
