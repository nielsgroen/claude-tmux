//! Scroll state management with center-locked selection behavior.
//!
//! As the user scrolls down, the selection stays centered in the visible area
//! until the bottom items become visible, then the selection moves to the
//! bottom portion of the view.

use ratatui::widgets::ListState;

/// Manages scroll state for a list with center-locked scrolling.
pub struct ScrollState {
    /// The underlying ratatui ListState
    list_state: ListState,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollState {
    pub fn new() -> Self {
        Self {
            list_state: ListState::default(),
        }
    }

    /// Update the scroll state given the current selection and list dimensions.
    ///
    /// Returns a mutable reference to the underlying ListState for rendering.
    ///
    /// # Arguments
    /// * `selected` - The index of the currently selected item
    /// * `total_items` - Total number of items in the list
    /// * `visible_height` - Height of the visible area in rows
    pub fn update(
        &mut self,
        selected: usize,
        total_items: usize,
        visible_height: usize,
    ) -> &mut ListState {
        // Set the selected item
        self.list_state.select(Some(selected));

        // Compute and set centered offset
        let offset = Self::compute_centered_offset(selected, total_items, visible_height);
        *self.list_state.offset_mut() = offset;

        &mut self.list_state
    }

    /// Compute the scroll offset to keep selection centered.
    ///
    /// Behavior:
    /// - Selection stays in the middle of the visible area
    /// - At the top: selection can be above middle (no negative scroll)
    /// - At the bottom: selection can be below middle (don't scroll past end)
    fn compute_centered_offset(
        selected: usize,
        total_items: usize,
        visible_height: usize,
    ) -> usize {
        if visible_height == 0 || total_items == 0 {
            return 0;
        }

        // Target: keep selection in the middle of visible area
        let middle = visible_height / 2;

        // If selection is in top half, no scroll needed
        if selected <= middle {
            return 0;
        }

        // Calculate ideal offset to center the selection
        let ideal_offset = selected.saturating_sub(middle);

        // Max offset: don't scroll past the point where bottom items fill the view
        let max_offset = total_items.saturating_sub(visible_height);

        ideal_offset.min(max_offset)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_scroll_when_at_top() {
        // With 10 visible rows, middle is at index 5
        // Selection at 0-5 should have offset 0
        assert_eq!(ScrollState::compute_centered_offset(0, 20, 10), 0);
        assert_eq!(ScrollState::compute_centered_offset(3, 20, 10), 0);
        assert_eq!(ScrollState::compute_centered_offset(5, 20, 10), 0);
    }

    #[test]
    fn test_scroll_to_center_selection() {
        // Selection at 7 with 10 visible rows (middle = 5)
        // Should scroll by 2 to put selection at middle
        assert_eq!(ScrollState::compute_centered_offset(7, 20, 10), 2);
        assert_eq!(ScrollState::compute_centered_offset(10, 20, 10), 5);
    }

    #[test]
    fn test_max_scroll_at_bottom() {
        // With 20 items and 10 visible, max offset is 10
        // Selection at 18 would ideally want offset 13, but capped at 10
        assert_eq!(ScrollState::compute_centered_offset(18, 20, 10), 10);
        assert_eq!(ScrollState::compute_centered_offset(19, 20, 10), 10);
    }

    #[test]
    fn test_edge_cases() {
        // Empty list
        assert_eq!(ScrollState::compute_centered_offset(0, 0, 10), 0);
        // Zero height
        assert_eq!(ScrollState::compute_centered_offset(5, 20, 0), 0);
        // List smaller than visible area
        assert_eq!(ScrollState::compute_centered_offset(3, 5, 10), 0);
    }
}
