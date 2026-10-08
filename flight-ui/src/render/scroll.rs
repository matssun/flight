// SPDX-License-Identifier: MIT

/// First visible row so the selected rows `first..=last` stay on screen in a window of
/// `height` rows over `total` rows, scrolling only as far as needed.
pub fn scroll_offset(selected: Option<(usize, usize)>, height: usize, total: usize) -> usize {
    let Some((first, last)) = selected else {
        return 0;
    };
    let max_offset = total.saturating_sub(height);
    last.saturating_sub(height.saturating_sub(1))
        .min(first)
        .min(max_offset)
}

#[cfg(test)]
mod tests {
    use super::scroll_offset;

    #[test]
    fn no_scroll_when_it_fits_or_nothing_is_selected() {
        assert_eq!(scroll_offset(None, 10, 50), 0);
        assert_eq!(scroll_offset(Some((3, 3)), 10, 8), 0);
        assert_eq!(scroll_offset(Some((9, 9)), 10, 50), 0);
    }

    #[test]
    fn scrolls_just_enough_to_show_the_selection() {
        assert_eq!(scroll_offset(Some((10, 10)), 10, 50), 1);
        assert_eq!(scroll_offset(Some((49, 49)), 10, 50), 40);
    }

    #[test]
    fn a_two_line_card_is_shown_whole() {
        assert_eq!(scroll_offset(Some((10, 11)), 10, 50), 2);
    }
}
