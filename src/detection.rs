use crate::session::ClaudeCodeStatus;

/// Strip ANSI escape sequences from a string.
/// Handles CSI sequences (ESC [ ... final byte) and OSC sequences (ESC ] ... BEL or ESC \).
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Escape sequence start
            if let Some(next) = chars.next() {
                match next {
                    '[' => {
                        // CSI sequence: consume until final byte (>= 0x40)
                        while let Some(c) = chars.next() {
                            if c as u8 >= 0x40 {
                                break;
                            }
                        }
                    }
                    ']' => {
                        // OSC sequence: consume until BEL (0x07) or ESC \
                        while let Some(c) = chars.next() {
                            if c == '\x07' {
                                break;
                            } else if c == '\x1b' {
                                // Check for ST (ESC \)
                                if let Some(next2) = chars.next() {
                                    if next2 == '\\' {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    _ => {
                        // Other escape sequences (ESC followed by single char) - already consumed next, ignore
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Detect input field: prompt line (❯) with border directly above it.
/// Returns the index of the prompt line if found. Searches from bottom to find the
/// most recent (current) prompt, not an old one in scrollback.
fn find_input_field_line(lines: &[&str]) -> Option<usize> {
    // Iterate from bottom up to find the most recent prompt
    for (i, line) in lines.iter().enumerate().rev() {
        if line.contains('❯') {
            // Check if line above is a border
            if i > 0 && lines[i - 1].contains('─') {
                return Some(i);
            }
        }
    }
    None
}

/// Check if content contains numbered options (e.g., "1. Yes", "2) No")
/// This indicates Claude is waiting for user input from a list of choices.
fn has_numbered_options(lines: &[&str], start: usize, end: usize) -> bool {
    for line in lines[start..=end].iter() {
        // Trim whitespace and strip ANSI escape sequences
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let no_ansi = strip_ansi_codes(trimmed);
        if no_ansi.is_empty() {
            continue;
        }

        // Skip prompt marker '❯' if present at the start, then trim again
        let content = if let Some(rest) = no_ansi.strip_prefix('❯') {
            rest.trim_start()
        } else {
            &no_ansi
        };

        if content.is_empty() {
            continue;
        }

        let mut chars = content.chars();
        let mut has_digit = false;

        while let Some(c) = chars.next() {
            if c.is_ascii_digit() {
                has_digit = true;
            } else if (c == '.' || c == ')') && has_digit {
                // After the separator, there must be non-whitespace content
                let rest: String = chars.collect();
                if !rest.trim_start().is_empty() {
                    return true;
                }
                break;
            } else {
                // Not a digit and not a separator after digits; not a match
                break;
            }
        }
    }
    false
}

/// Check if content contains a working animation indicator above the prompt.
/// Claude shows an animation line above the prompt while actively thinking.
/// These lines typically describe an action ending in "ing" followed by an ellipsis.
///
/// Examples:
/// - "✶ Zigzagging… (38s · ↓ 2.0k tokens)" - Working
/// - "· Finagling…" - Working
/// - "○ Spinning… (10s)" - Working
/// - "✻ Cogitated for 2m 32s" - Idle (past tense, no ellipsis)
fn has_working_indicator(lines: &[&str], prompt_idx: usize) -> bool {
    // Search in lines above the prompt (including the border line)
    // Check up to 20 lines above to find animation indicators
    let start = prompt_idx.saturating_sub(20);
    let search_range = &lines[start..prompt_idx];

    for line in search_range {
        let stripped = strip_ansi_codes(line);
        let lower = stripped.to_lowercase();
        // Look for an "ing" substring (typical in verbs) AND the special ellipsis character (…).
        // The ellipsis indicates the activity is still in progress.
        if lower.contains("ing") && stripped.contains('…') {
            return true;
        }
    }

    false
}

pub fn detect_status(content: &str) -> ClaudeCodeStatus {
    let lines: Vec<&str> = content.lines().collect();

    // Primary: look for input field with border above (proper prompt)
    if let Some(i) = find_input_field_line(&lines) {
        // Check for working indicator (ellipsis above prompt)
        if has_working_indicator(&lines, i) {
            return ClaudeCodeStatus::Working;
        }

        // Check if it's a prompt with numbered choices (waiting for input)
        let start = i.saturating_sub(2);
        let end = std::cmp::min(i + 20, lines.len().saturating_sub(1));

        // Check for "interrupt" message (old method - fallback)
        let mut has_interrupt = false;
        for idx in start..=end {
            let line = lines[idx];
            if line.contains("interrupt") || line.contains("esc to interrupt") || line.contains("esc t") {
                has_interrupt = true;
                break;
            }
        }

        if has_interrupt {
            return ClaudeCodeStatus::Working;
        }

        if has_numbered_options(&lines, start, end) {
            return ClaudeCodeStatus::WaitingInput;
        }

        // No working indicator and no numbered choices = Idle (prompt ready for input)
        return ClaudeCodeStatus::Idle;
    }

    // Fallback: detect numbered-choice prompts without a border
    // Some prompts (like permission dialogs) may not have the typical border
    if let Some(i) = lines.iter().rposition(|line| line.contains('❯')) {
        let start = i.saturating_sub(2);
        let end = std::cmp::min(i + 20, lines.len().saturating_sub(1));
        if has_numbered_options(&lines, start, end) {
            return ClaudeCodeStatus::WaitingInput;
        }
    }

    // No input field - check for simple permission prompt [y/n]
    if content.contains("[y/n]") || content.contains("[Y/n]") {
        return ClaudeCodeStatus::WaitingInput;
    }

    ClaudeCodeStatus::Unknown
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
    fn test_working_with_esc() {
        // New format: esc to interrupt
        let content = "* (esc to interrupt)\n─────\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_with_escape_uppercase() {
        let content = "Ctrl+C to interrupt\n─────\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_esc_on_prompt_line() {
        // Interrupt message on same line as prompt
        let content = "─────\n❯ esc to interrupt";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_esc_above_border() {
        // Simple "esc to interrupt" without wrapper chars
        let content = "esc to interrupt\n─────\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_idle() {
        // Border directly above prompt
        let content = "● Done\n─────\n❯ hello";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Idle);
    }

    #[test]
    fn test_false_positive_from_scrollback() {
        // Old output contains "interrupt", but current prompt does NOT show interrupt message.
        // Should be Idle, not Working - verifies we only search near prompt
        let content = "Some old output mentioning interrupt\n\n─────\n❯ ready";
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
    fn test_interrupt_below_prompt() {
        // Interrupt message appears below the prompt/separator (real-world layout)
        let content = "────────────────────\n❯ \n────────────────────\n  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_truncated_interrupt() {
        // Truncated message due to narrow window: "esc to…"
        let content = "────────────────────\n❯ \n────────────────────\n  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to…";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_waiting_input_numbered_choices() {
        // Multiple choice prompt with numbered options
        let content = "\nDo you want to proceed?\n─────\n❯ 1. Yes\n  2. Yes, and don't ask again for: env\n  3. No\n\nEsc to cancel · Tab to amend · ctrl+e to explain";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_waiting_input_numbered_choices_below_prompt() {
        // Options appear below the prompt line
        let content = "─────\n❯ Select action:\n1. Continue\n2. Cancel\n3. Help";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_waiting_input_parenthesis_numbers() {
        // Test with closing parenthesis instead of period
        let content = "─────\n❯ Choose:\n1) Accept\n2) Decline";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_waiting_input_numbered_choices_no_border() {
        // Prompt with numbered choices but no border line - should still be detected via fallback
        let content = "Do you want to proceed?\n❯ 1. Yes\n2. No\n3. Maybe";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_no_border_no_numbered_options() {
        // Prompt line without border and without numbered choices - should remain Unknown
        let content = "❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Unknown);
    }

    #[test]
    fn test_waiting_input_with_ansi_codes() {
        // Simulate colored numbered options with ANSI escape codes
        let content = "Do you want to proceed?\n\x1b[32m❯\x1b[0m \x1b[1;32m1.\x1b[0m Yes\n\x1b[1;32m2.\x1b[0m No\n\x1b[1;32m3.\x1b[0m Maybe";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_working_with_zigzagging_animation() {
        // Zigzagging with diamond symbol and ellipsis
        let content = "✶ Zigzagging… (38s · ↓ 2.0k tokens)\n─────\n❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_with_zigzagging_dot() {
        // Zigzagging with middle dot symbol
        let content = "· Zigzagging… (1m 14s · ↓ 3.5k tokens)\n─────\n❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_with_truncated_zigzagging() {
        // Zigzagging that might get truncated depending on screen width
        let content = "Zigzagging… (38s · ↓ 2.0k)\n─────\n❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_idle_with_cogitated() {
        // Completed thinking with elapsed time (no ellipsis)
        let content = "✻ Cogitated for 2m 32s\n─────\n❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Idle);
    }

    #[test]
    fn test_working_with_spinning() {
        // Other possible animation types
        let content = "○ Spinning… (10s)\n─────\n❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_multiple_lines_above() {
        // Working indicator several lines above prompt
        let content = "✶ Zigzagging… (38s · ↓ 2.0k tokens)\n\n─────\n❯ ready";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_working_with_implementing() {
        // Longer verb with ellipsis at end (ellipsis appears after many chars)
        let content = "✽ Implementing separate app token cache fix…\n───────────────────────────────────────────────\n❯ ";
        assert_eq!(detect_status(content), ClaudeCodeStatus::Working);
    }

    #[test]
    fn test_waiting_input_with_prompt_marker_on_same_line() {
        // Your first example: prompt marker on same line as first option
        let content = " Do you want to proceed?\n❯ 1. Yes\n  2. No\n\nEsc to cancel · Tab to amend · ctrl+e to explain";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }

    #[test]
    fn test_waiting_input_compound_command_example() {
        // Your second example: compound command approval
        let content = "──────────────────────────────────────────────────────\n Bash command\n\n   cd \"/Users/nathanmathis/src/xcode-projects/FroggyTV\" && git checkout HEAD --\n   FroggyTV/Services/IRCMessageHandler.swift\n   Restore IRCMessageHandler from git\n\n Compound commands with cd and git require approval\n to prevent bare repository attacks\n\n Do you want to proceed?\n❯ 1. Yes\n  2. No\n\n Esc to cancel · Tab to amend · ctrl+e to explain";
        assert_eq!(detect_status(content), ClaudeCodeStatus::WaitingInput);
    }
}
