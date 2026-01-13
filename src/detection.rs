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
