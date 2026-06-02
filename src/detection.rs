use crate::session::ClaudeCodeStatus;

/// Return content up to and including the input prompt boundary (❯ line with
/// ─ border above it), stripping any status bar lines below. If no prompt
/// boundary is found, return the full content unchanged. This works regardless
/// of whether the user has 0, 1, 2, or more status bar lines.
pub fn content_above_status_bar(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    for i in (0..lines.len()).rev() {
        if lines[i].contains('❯') && i > 0 && lines[i - 1].contains('─') {
            let end = lines[..=i].iter().map(|l| l.len() + 1).sum::<usize>() - 1;
            return &content[..end];
        }
    }
    content
}

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
    fn test_content_above_status_bar_two_line_bar() {
        let content = "some output\n─────\n❯ hello\nINS | ~/project | Opus 4.6\n(◠‿◠) 0% | $0.00 | 10m 43s";
        assert_eq!(content_above_status_bar(content), "some output\n─────\n❯ hello");
    }

    #[test]
    fn test_content_above_status_bar_one_line_bar() {
        let content = "some output\n─────\n❯ hello\nstatus: idle";
        assert_eq!(content_above_status_bar(content), "some output\n─────\n❯ hello");
    }

    #[test]
    fn test_content_above_status_bar_no_bar() {
        let content = "some output\n─────\n❯ hello";
        assert_eq!(content_above_status_bar(content), "some output\n─────\n❯ hello");
    }

    #[test]
    fn test_content_above_status_bar_no_prompt() {
        let content = "random stuff\nno prompt here";
        assert_eq!(content_above_status_bar(content), "random stuff\nno prompt here");
    }
}
