//! Path and text completion utilities
//!
//! Provides filesystem path completion for input fields.

use std::path::{Path, PathBuf};

/// Result of path completion operation
#[derive(Debug, Default)]
pub struct PathCompletion {
    /// All matching path suggestions (full paths, display-ready with ~ for home)
    pub suggestions: Vec<String>,
    /// Ghost text suffix to display after current input (just the completion part)
    pub ghost_text: Option<String>,
}

/// Complete a partial path string
///
/// # Arguments
/// * `partial` - The partial path typed by the user (may include ~)
///
/// # Returns
/// A `PathCompletion` with matching suggestions and ghost text
pub fn complete_path(partial: &str) -> PathCompletion {
    let partial = partial.trim();

    if partial.is_empty() {
        // Empty input: show current directory contents
        return complete_in_directory(Path::new("."), "", true);
    }

    // Expand ~ to home directory for filesystem operations
    let (expanded, uses_tilde) = expand_for_completion(partial);
    let expanded_path = Path::new(&expanded);

    // If path ends with /, list that directory
    if partial.ends_with('/') {
        if expanded_path.is_dir() {
            return complete_in_directory(expanded_path, "", uses_tilde);
        } else {
            return PathCompletion::default();
        }
    }

    // Split into directory and prefix to match
    let (dir, prefix) = if let Some(parent) = expanded_path.parent() {
        let filename = expanded_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        (parent.to_path_buf(), filename.to_string())
    } else {
        (PathBuf::from("."), expanded.clone())
    };

    if !dir.exists() {
        return PathCompletion::default();
    }

    complete_in_directory(&dir, &prefix, uses_tilde)
}

/// Complete entries in a directory matching a prefix
fn complete_in_directory(dir: &Path, prefix: &str, uses_tilde: bool) -> PathCompletion {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return PathCompletion::default(),
    };

    let prefix_lower = prefix.to_lowercase();
    let home_dir = dirs::home_dir();

    let mut matches: Vec<(String, bool)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();

            // Filter: must start with prefix (case-insensitive)
            if !prefix.is_empty() && !name_lower.starts_with(&prefix_lower) {
                return None;
            }

            // Skip hidden files unless prefix starts with .
            if name.starts_with('.') && !prefix.starts_with('.') {
                return None;
            }

            let full_path = dir.join(&name);
            let is_dir = full_path.is_dir();

            // Format path for display
            let display_path = format_display_path(&full_path, uses_tilde, &home_dir, is_dir);

            Some((display_path, is_dir))
        })
        .collect();

    // Sort: directories first, then alphabetically
    matches.sort_by(|a, b| {
        match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        }
    });

    let suggestions: Vec<String> = matches.into_iter().map(|(path, _)| path).collect();

    // Calculate ghost text (common suffix of first suggestion)
    let ghost_text = calculate_ghost_text(prefix, &suggestions);

    PathCompletion {
        suggestions,
        ghost_text,
    }
}

/// Calculate ghost text suffix based on current input and suggestions
fn calculate_ghost_text(prefix: &str, suggestions: &[String]) -> Option<String> {
    if suggestions.is_empty() {
        return None;
    }

    // Get the first suggestion and extract the suffix after the prefix
    let first = &suggestions[0];

    // Find where the prefix matches in the suggestion (at the filename part)
    let first_lower = first.to_lowercase();
    let prefix_lower = prefix.to_lowercase();

    // Find the last component that matches our prefix
    if let Some(last_sep) = first.rfind('/') {
        let filename = &first[last_sep + 1..];
        let filename_lower = filename.to_lowercase();

        if filename_lower.starts_with(&prefix_lower) {
            // Return the suffix of the filename (what would be added)
            let suffix = &filename[prefix.len()..];
            if !suffix.is_empty() {
                return Some(suffix.to_string());
            }
        }
    } else if first_lower.starts_with(&prefix_lower) {
        // No separator, simple case
        let suffix = &first[prefix.len()..];
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }

    None
}

/// Format a path for display, using ~ for home directory
fn format_display_path(
    path: &Path,
    uses_tilde: bool,
    home_dir: &Option<PathBuf>,
    is_dir: bool,
) -> String {
    let mut display = if uses_tilde {
        if let Some(home) = home_dir {
            if let Ok(stripped) = path.strip_prefix(home) {
                format!("~/{}", stripped.display())
            } else {
                path.display().to_string()
            }
        } else {
            path.display().to_string()
        }
    } else {
        path.display().to_string()
    };

    // Append / to directories
    if is_dir && !display.ends_with('/') {
        display.push('/');
    }

    display
}

/// Expand ~ to home directory, returning (expanded_path, used_tilde)
fn expand_for_completion(path: &str) -> (String, bool) {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return (format!("{}/{}", home.display(), stripped), true);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return (home.display().to_string(), true);
        }
    }
    (path.to_string(), path.starts_with('~'))
}

/// Calculate ghost text for a branch completion
/// Returns the suffix that would be added to complete to the target branch
pub fn branch_ghost_text(input: &str, branches: &[&str], selected: Option<usize>) -> Option<String> {
    if branches.is_empty() {
        return None;
    }

    // Use selected branch or first match
    let target = if let Some(idx) = selected {
        branches.get(idx)?
    } else {
        branches.first()?
    };

    let input_lower = input.to_lowercase();
    let target_lower = target.to_lowercase();

    // If input is a prefix of target, return the suffix
    if target_lower.starts_with(&input_lower) {
        let suffix = &target[input.len()..];
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_for_completion() {
        let (expanded, uses_tilde) = expand_for_completion("~/test");
        assert!(uses_tilde);
        assert!(expanded.contains("test"));

        let (expanded, uses_tilde) = expand_for_completion("/absolute/path");
        assert!(!uses_tilde);
        assert_eq!(expanded, "/absolute/path");
    }

    #[test]
    fn test_branch_ghost_text() {
        let branches = vec!["main", "feature/login", "feature/signup"];

        assert_eq!(
            branch_ghost_text("feat", &branches, None),
            Some("ure/login".to_string())
        );

        assert_eq!(
            branch_ghost_text("feature/", &branches, Some(1)),
            Some("signup".to_string())
        );

        assert_eq!(branch_ghost_text("nonexistent", &branches, None), None);
    }
}
