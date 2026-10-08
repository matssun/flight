// SPDX-License-Identifier: MIT

use ratatui::layout::{Constraint, Layout, Rect};

/// Side by side from this width.
const WIDE_FROM: u16 = 100;
/// Stacked needs room for a list and a preview; shorter screens show the list alone.
const STACKED_FROM_HEIGHT: u16 = 20;
/// Below this width a list row is a small card (two lines) instead of one line.
pub const CARD_BELOW: u16 = 44;
/// The legend line above the hints needs this much height.
const LEGEND_FROM_HEIGHT: u16 = 14;
/// Smaller than this and there is nothing sensible to draw.
pub const MIN_WIDTH: u16 = 20;
pub const MIN_HEIGHT: u16 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    /// List on the left, preview on the right.
    Wide,
    /// List above, preview below.
    Stacked,
    /// The list alone.
    ListOnly,
}

pub fn layout_kind(width: u16, height: u16) -> LayoutKind {
    if width >= WIDE_FROM {
        LayoutKind::Wide
    } else if height >= STACKED_FROM_HEIGHT {
        LayoutKind::Stacked
    } else {
        LayoutKind::ListOnly
    }
}

/// Where each part of the dashboard goes.
#[derive(Debug, Clone, Copy)]
pub struct Areas {
    pub header: Rect,
    pub list: Rect,
    pub preview: Option<Rect>,
    pub footer: Rect,
}

pub fn areas(area: Rect) -> Areas {
    let kind = layout_kind(area.width, area.height);
    let footer_h = if area.height >= LEGEND_FROM_HEIGHT {
        2
    } else {
        1
    };
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(footer_h),
    ])
    .areas(area);
    let (list, preview) = match kind {
        LayoutKind::Wide => {
            let [l, r] =
                Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)])
                    .areas(body);
            (l, Some(r))
        }
        LayoutKind::Stacked => {
            let [t, b] = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(body);
            (t, Some(b))
        }
        LayoutKind::ListOnly => (body, None),
    };
    Areas {
        header,
        list,
        preview,
        footer,
    }
}
