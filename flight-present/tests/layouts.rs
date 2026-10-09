// SPDX-License-Identifier: MIT

//! Examples: what the layouts of ADR-011 come to on a real terminal size.

use flight_present::{
    cycle, neighbor, saved, solve, Axis, Child, Direction, Layout, LayoutError, Placement, Rect,
    Region, Style,
};
use flight_state::SurfaceId;

fn id(s: &str) -> SurfaceId {
    SurfaceId::new(s)
}

fn leaf(s: &str) -> Region {
    Region::Surface(id(s))
}

fn screen() -> Rect {
    Rect::new(0, 0, 100, 30)
}

fn side_by_side() -> Layout {
    Layout::new(
        Region::Split {
            axis: Axis::Across,
            children: vec![Child::new(1, leaf("agent")), Child::new(1, leaf("shell"))],
        },
        id("agent"),
    )
    .unwrap()
}

#[test]
fn a_single_surface_fills_the_terminal() {
    let l = Layout::single(id("agent"));
    let s = solve(&l, screen(), &Style::default());
    assert_eq!(s.tiles.len(), 1);
    assert_eq!(s.tiles[0].area, screen());
    assert!(s.tiles[0].focused && s.dividers.is_empty() && s.hidden.is_empty());
}

#[test]
fn two_surfaces_side_by_side_share_the_width_with_a_line_between() {
    let s = solve(&side_by_side(), screen(), &Style::default());
    let agent = s.tile(&id("agent")).unwrap().area;
    let shell = s.tile(&id("shell")).unwrap().area;
    assert_eq!(agent, Rect::new(0, 0, 50, 30));
    assert_eq!(s.dividers.len(), 1);
    assert_eq!(s.dividers[0].area, Rect::new(50, 0, 1, 30));
    assert_eq!(shell, Rect::new(51, 0, 49, 30));
    assert_eq!(s.focused().unwrap().surface, id("agent"));
}

#[test]
fn shares_are_proportions_and_every_cell_is_used() {
    let l = Layout::new(
        Region::Split {
            axis: Axis::Across,
            children: vec![Child::new(1, leaf("a")), Child::new(3, leaf("b"))],
        },
        id("a"),
    )
    .unwrap();
    let s = solve(&l, Rect::new(0, 0, 81, 10), &Style::default());
    // 80 cells after the line: a quarter and three quarters.
    assert_eq!(s.tile(&id("a")).unwrap().area.cols, 20);
    assert_eq!(s.tile(&id("b")).unwrap().area.cols, 60);
}

#[test]
fn splits_nest_agent_left_and_shell_over_a_third_on_the_right() {
    let l = Layout::new(
        Region::Split {
            axis: Axis::Across,
            children: vec![
                Child::new(2, leaf("agent")),
                Child::new(
                    1,
                    Region::Split {
                        axis: Axis::Down,
                        children: vec![Child::new(1, leaf("shell")), Child::new(1, leaf("logs"))],
                    },
                ),
            ],
        },
        id("agent"),
    )
    .unwrap();
    let s = solve(&l, screen(), &Style::default());
    assert_eq!(s.tiles.len(), 3);
    assert_eq!(s.dividers.len(), 2);
    let shell = s.tile(&id("shell")).unwrap().area;
    let logs = s.tile(&id("logs")).unwrap().area;
    assert_eq!(shell.cols, logs.cols);
    assert_eq!(
        shell.bottom().saturating_add(1),
        logs.y,
        "one line between them"
    );
    assert_eq!(logs.bottom(), 30);
}

#[test]
fn tabs_show_one_and_the_others_hold_no_tile() {
    let l = Layout::new(
        Region::Tabs {
            active: 0,
            tabs: vec![leaf("agent"), leaf("shell"), leaf("logs")],
        },
        id("agent"),
    )
    .unwrap();
    let s = solve(&l, screen(), &Style::default());
    assert_eq!(s.tab_bars.len(), 1);
    assert_eq!(s.tab_bars[0].area, Rect::new(0, 0, 100, 1));
    assert_eq!(
        s.tab_bars[0].tabs,
        vec![id("agent"), id("shell"), id("logs")]
    );
    assert_eq!(s.tiles.len(), 1);
    assert_eq!(s.tiles[0].area, Rect::new(0, 1, 100, 29));
    assert_eq!(s.hidden, vec![id("shell"), id("logs")]);
    // Showing another tab is a different layout; the hidden surfaces were never touched.
    let l2 = l.focus_on(&id("logs")).unwrap();
    let s2 = solve(&l2, screen(), &Style::default());
    assert_eq!(s2.tiles[0].surface, id("logs"));
    assert_eq!(s2.hidden, vec![id("agent"), id("shell")]);
}

#[test]
fn a_terminal_too_small_for_everything_shows_the_focused_surface_alone() {
    let l = side_by_side();
    let tiny = Rect::new(0, 0, 30, 10);
    let s = solve(&l, tiny, &Style::default());
    assert!(s.squeezed);
    assert_eq!(s.tiles.len(), 1);
    assert_eq!(s.tiles[0].surface, id("agent"));
    assert_eq!(s.tiles[0].area, tiny);
    assert_eq!(s.hidden, vec![id("shell")]);
    // Growing it brings the layout back; nothing was changed meanwhile.
    assert!(!solve(&l, screen(), &Style::default()).squeezed);
}

#[test]
fn focus_moves_by_direction_and_in_reading_order() {
    let l = Layout::new(
        Region::Split {
            axis: Axis::Across,
            children: vec![
                Child::new(1, leaf("agent")),
                Child::new(
                    1,
                    Region::Split {
                        axis: Axis::Down,
                        children: vec![Child::new(1, leaf("shell")), Child::new(1, leaf("logs"))],
                    },
                ),
            ],
        },
        id("agent"),
    )
    .unwrap();
    let s = solve(&l, screen(), &Style::default());
    assert_eq!(
        neighbor(&s, &id("agent"), Direction::Right),
        Some(id("shell"))
    );
    assert_eq!(
        neighbor(&s, &id("shell"), Direction::Down),
        Some(id("logs"))
    );
    assert_eq!(
        neighbor(&s, &id("logs"), Direction::Left),
        Some(id("agent"))
    );
    assert_eq!(neighbor(&s, &id("agent"), Direction::Left), None);
    assert_eq!(neighbor(&s, &id("agent"), Direction::Up), None);
    assert_eq!(cycle(&s, &id("agent"), true), Some(id("shell")));
    assert_eq!(cycle(&s, &id("logs"), true), Some(id("agent")), "wraps");
    assert_eq!(cycle(&s, &id("agent"), false), Some(id("logs")));
}

#[test]
fn editing_a_layout_never_names_a_surface_twice_or_loses_the_keyboard() {
    let one = Layout::single(id("agent"));
    let two = one
        .split(&id("agent"), Axis::Across, id("shell"), Placement::After)
        .unwrap();
    assert_eq!(two.visible(), vec![&id("agent"), &id("shell")]);
    assert_eq!(
        two.focus(),
        &id("shell"),
        "the new surface has the keyboard"
    );
    // Splitting the same way again adds a sibling instead of another level.
    let three = two
        .split(&id("shell"), Axis::Across, id("logs"), Placement::After)
        .unwrap();
    assert_eq!(three.root().depth(), 2);
    assert!(three
        .split(&id("agent"), Axis::Down, id("shell"), Placement::After)
        .is_err());
    // Removing hides, closes up, and moves the keyboard to the neighbour.
    let back = three.remove(&id("shell")).unwrap();
    assert_eq!(back.visible(), vec![&id("agent"), &id("logs")]);
    assert_eq!(
        back.focus(),
        &id("logs"),
        "removing another surface leaves the keyboard alone"
    );
    let lost = three.remove(&id("logs")).unwrap();
    assert_eq!(lost.focus(), &id("shell"), "the one just before takes over");
    assert_eq!(
        back.remove(&id("agent"))
            .unwrap()
            .remove(&id("logs"))
            .unwrap_err(),
        LayoutError::Empty
    );
    assert!(matches!(
        back.remove(&id("nope")).unwrap_err(),
        LayoutError::Unknown(_)
    ));
}

#[test]
fn resizing_moves_share_between_neighbours_and_keeps_both_alive() {
    let l = side_by_side();
    let wider = l.resize(&id("agent"), Axis::Across, 20).unwrap();
    let s = solve(&wider, screen(), &Style::default());
    assert!(s.tile(&id("agent")).unwrap().area.cols > 55);
    let extreme = l.resize(&id("agent"), Axis::Across, 10_000).unwrap();
    let s = solve(&extreme, screen(), &Style::default());
    assert!(s.tile(&id("shell")).unwrap().area.cols >= 1);
    // A direction nothing splits in is a no-op, not an error.
    assert_eq!(l.resize(&id("agent"), Axis::Down, 5).unwrap(), l);
}

#[test]
fn adding_a_tab_next_to_a_surface_and_removing_it_again() {
    let l = Layout::single(id("agent"))
        .add_tab(&id("agent"), id("shell"))
        .unwrap();
    assert_eq!(l.focus(), &id("shell"));
    assert_eq!(l.visible(), vec![&id("shell")]);
    assert_eq!(l.surfaces().len(), 2);
    let l = l.add_tab(&id("agent"), id("logs")).unwrap();
    assert_eq!(l.surfaces().len(), 3);
    let back = l.remove(&id("logs")).unwrap().remove(&id("shell")).unwrap();
    assert_eq!(back, Layout::single(id("agent")));
}

#[test]
fn a_saved_layout_reads_back_the_same_and_fits_the_surfaces_that_exist() {
    let l = side_by_side()
        .split(&id("shell"), Axis::Down, id("logs"), Placement::After)
        .unwrap();
    let text = saved::to_toml(&l).unwrap();
    assert_eq!(saved::from_toml(&text, |_| true).unwrap(), l);
    // The logs surface is gone: the tree closes up around it.
    let fitted = saved::from_toml(&text, |s| s.as_str() != "logs").unwrap();
    assert_eq!(fitted.visible(), vec![&id("agent"), &id("shell")]);
    // Nothing named exists: no layout, and no surface is invented.
    assert_eq!(
        saved::from_toml(&text, |_| false).unwrap_err(),
        LayoutError::Empty
    );
}

#[test]
fn a_saved_layout_is_read_with_suspicion() {
    let l = side_by_side();
    let good = saved::to_toml(&l).unwrap();
    for bad in [
        String::new(),
        "version = 99\nfocus = \"x\"\n".to_owned(),
        good.replacen("version = 1", "version = 2", 1),
        good.replace("agent", "a b"),
        "x".repeat(saved::MAX_BYTES + 1),
        good.replace("weight = 1", "weight = 0"),
        good.replace("weight = 1", "weight = 60000"),
    ] {
        assert!(saved::from_toml(&bad, |_| true).is_err(), "{bad:.80}");
    }
    // The same surface twice is refused, not shown twice.
    let twice = good.replace("shell", "agent");
    assert!(matches!(
        saved::from_toml(&twice, |_| true).unwrap_err(),
        LayoutError::Duplicate(_)
    ));
    // A focus that is not showing falls back to the first showing surface.
    let stale = good.replacen("focus = \"agent\"", "focus = \"gone\"", 1);
    assert_eq!(
        saved::from_toml(&stale, |_| true).unwrap().focus(),
        &id("agent")
    );
}

#[test]
fn nesting_deeper_than_the_limit_is_refused() {
    let mut r = leaf("s0");
    for i in 1..=8 {
        r = Region::Split {
            axis: if i % 2 == 0 { Axis::Across } else { Axis::Down },
            children: vec![Child::new(1, r), Child::new(1, leaf(&format!("s{i}")))],
        };
    }
    assert_eq!(Layout::new(r, id("s0")).unwrap_err(), LayoutError::TooDeep);
}

#[test]
fn replacing_a_surface_keeps_its_place_share_and_the_keyboard() {
    let l = side_by_side()
        .resize(&id("agent"), Axis::Across, 20)
        .unwrap();
    let swapped = l.replace(&id("agent"), id("notes")).unwrap();
    assert!(!swapped.contains(&id("agent")));
    assert_eq!(swapped.focus(), &id("notes"));
    let before = solve(&l, screen(), &Style::default());
    let after = solve(&swapped, screen(), &Style::default());
    assert_eq!(
        before.tile(&id("agent")).unwrap().area,
        after.tile(&id("notes")).unwrap().area
    );
    // A surface already in the layout cannot be put in twice.
    assert!(matches!(
        l.replace(&id("agent"), id("shell")).unwrap_err(),
        LayoutError::Duplicate(_)
    ));
    // Replacing one that does not have the keyboard leaves the keyboard alone.
    assert_eq!(
        l.replace(&id("shell"), id("logs")).unwrap().focus(),
        &id("agent")
    );
}

#[test]
fn stepping_through_tabs_wraps_and_a_layout_without_tabs_does_not_change() {
    let l = Layout::new(
        Region::Tabs {
            active: 0,
            tabs: vec![leaf("agent"), leaf("shell"), leaf("logs")],
        },
        id("agent"),
    )
    .unwrap();
    let next = l.step_tab(true);
    assert_eq!(next.focus(), &id("shell"));
    assert_eq!(next.step_tab(true).focus(), &id("logs"));
    assert_eq!(next.step_tab(true).step_tab(true).focus(), &id("agent"));
    assert_eq!(l.step_tab(false).focus(), &id("logs"));
    assert_eq!(side_by_side().step_tab(true), side_by_side());
}
