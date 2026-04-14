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
    // Step 1: Detect input field by its visual structure
    if has_input_field(content) {
        // Step 2: Check if interruptable
        if content.contains("ctrl+c") && content.contains("to interrupt") {
            return ClaudeCodeStatus::Working;
        }
        // Check for active spinner/thinking states (e.g. "· Channeling…")
        if has_active_spinner(content) {
            return ClaudeCodeStatus::Working;
        }
        return ClaudeCodeStatus::Idle;
    }

    // No input field - check for active work (thinking/streaming state)
    if content.contains("ctrl+c") && content.contains("to interrupt") {
        return ClaudeCodeStatus::Working;
    }

    if has_active_spinner(content) {
        return ClaudeCodeStatus::Working;
    }

    // Check for permission prompt
    if content.contains("[y/n]") || content.contains("[Y/n]") {
        return ClaudeCodeStatus::WaitingInput;
    }

    ClaudeCodeStatus::Unknown
}

/// Check for active spinner/thinking indicators in Claude Code output.
/// Returns true if Claude appears to be actively processing.
///
/// Active states always include `…` (U+2026) in their status line regardless
/// of which spinner character precedes it ("· Channeling…", "✻ Bootstrapping…",
/// etc.). Braille spinner characters are also detected as a fallback.
/// The done state ("● Done (...)") never contains `…`.
fn has_active_spinner(content: &str) -> bool {
    const BRAILLE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    content.lines().any(|line| {
        line.contains('…') || BRAILLE.iter().any(|&c| line.contains(c))
    })
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
}
