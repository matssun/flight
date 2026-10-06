// SPDX-License-Identifier: MIT

/// First visible row so `selected` stays on screen in a window of `height` rows over
/// `total` rows, scrolling only as far as needed.
pub fn scroll_offset(selected: Option<usize>, height: usize, total: usize) -> usize {
    let Some(sel) = selected else { return 0 };
    let max_offset = total.saturating_sub(height);
    sel.saturating_sub(height.saturating_sub(1)).min(max_offset)
}

#[cfg(test)]
mod tests {
    use super::scroll_offset;

    #[test]
    fn no_scroll_when_it_fits_or_nothing_is_selected() {
        assert_eq!(scroll_offset(None, 10, 50), 0);
        assert_eq!(scroll_offset(Some(3), 10, 8), 0);
        assert_eq!(scroll_offset(Some(9), 10, 50), 0);
    }

    #[test]
    fn scrolls_just_enough_to_show_the_selection() {
        assert_eq!(scroll_offset(Some(10), 10, 50), 1);
        assert_eq!(scroll_offset(Some(49), 10, 50), 40);
    }
}
