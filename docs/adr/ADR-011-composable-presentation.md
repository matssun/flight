<!-- SPDX-License-Identifier: MIT -->

# ADR-011: Composable presentation over persistent surfaces

Status: model implemented (`flight-present`); compositor and multi-surface session follow (see "Order"). Builds on ADR-007 (surfaces), ADR-009 (attachments, views, the surface session).

## The requirement

Show one surface, two side by side, horizontal and vertical splits, nested regions, tabs, more than two surfaces and kinds of resource that are not terminals yet; each region with its own size, focus and input; layouts that can be saved and restored. And: **a layout never decides what a surface is or how long it lives**, and a stale `capture-pane` is not an acceptable stand-in for an interactive surface.

## What already holds (ADR-009)

- A surface is a tmux window and lives in the backend. Hiding it ends nothing.
- An attachment (one terminal stream, one node-side tmux client, one *view* session with its own current window) is opened to show a surface and let go to stop showing it, and a hidden surface holds none. Views make two attachments to two surfaces of one workspace independent: separate current windows, separate sizes, keys that can only reach the window each shows. The wire protocol names a surface and gets an attachment; it does not know about layouts.
- The surface session owns the keyboard as an ordered, bounded log.

So simultaneous presentation needs no new transport. What it needs is a client that (1) decides what is on screen, (2) keeps one attachment per showing surface, (3) turns each attachment's bytes into something it can place in a region, and (4) routes the keyboard to one of them.

## Do we need our own terminal rendering model? Investigation

Composing two interactive byte streams into one terminal requires knowing what each stream has drawn: a stream is a program driving a screen with cursor addressing, scrolling regions, colours, alternate screens. Raw bytes cannot be placed in half a terminal. Options: delegate to tmux splits (surfaces become panes of one window: gives up independent presentation and, as found in ADR-009 and in the CI failure that led to its test, panes of one window share their active pane), show captures (stale, not interactive; rejected by the requirement), or emulate each stream into a screen model and draw the models.

Candidates for the screen model, all checked against the same real input: the output of a real tmux client (a pty, `tmux attach`, 100x30) for a pane that printed text with bold/reverse/underline, 256-colour and truecolour backgrounds, CJK and emoji (double width), combining characters, box drawing, a tab, a line wider than the screen, and 40 lines of scrolling; the emulated rows compared with `tmux capture-pane` of the same pane (the status line excluded). The experiment is small enough to rerun (a scratch crate of about 100 lines; the parts that matter are kept as the compositor's fidelity test).

| | licence | size and deps | rows differing from tmux (29) | throughput (64 MB of text and SGR) | note |
|---|---|---|---|---|---|
| `vt100` 0.16 | MIT | small; `vte`, `unicode-width` | **0** | 350 ms (about 180 MB/s) | screen model with cell attributes, cursor, and the mode flags a compositor needs (alternate screen, application cursor keys, bracketed paste, mouse protocol) |
| `avt` 0.18 | Apache-2.0 | small | 0 | 690 ms | asciinema's; line-oriented API |
| `alacritty_terminal` 0.26 | Apache-2.0 | larger (grid, event loop types) | 0 | not measured | the most faithful, the heaviest to hold N of |
| `termwiz` 0.23 | MIT | large (wezterm's) | not run | not run | more than needed |
| `tui-term` 0.3 | MIT | widget over `vt100` | n/a | n/a | needs a newer ratatui than the repository's 0.29; the widget is about a hundred lines to write |

All three emulators tried reproduce a real tmux client's drawing exactly on this input, so fidelity does not choose between them; licence, size and the mode accessors do.

**Decision.** Flight does **not** write a terminal emulator. It uses `vt100` as the per-attachment screen model (MIT like the repository, smallest, the mode accessors the compositor needs) behind a narrow adapter so another crate can replace it, and writes only the thin part that is its own: placing screens in regions (a few hundred lines over ratatui's buffer, which the dashboard already uses), the cursor, focus and input routing. A screen model exists only for a showing surface: it is created when the attachment is, dropped when it is, and has no scrollback (tmux, inside, keeps the history; the model holds one screen of cells). Hidden surfaces cost nothing, as before.

## The layout model (`flight-present`)

Pure, no I/O, no clock, no dependency on the transport.

```
Layout  = { root: Region, focus: SurfaceId }
Region  = Surface(SurfaceId)
        | Split { axis: Across | Down, children: [{ weight, Region }] }
        | Tabs  { active, tabs: [Region] }
```

- **Identity.** Leaves are `SurfaceId`s. No tmux id, pane or window appears, and nothing in a layout is a process or a connection. Removing a leaf stops *showing* a surface; a layout that is dropped, replaced or lost leaves every surface running.
- **Invariants** (checked on construction and kept by every edit): at least one surface; none twice (it would need two attachments); no empty split or tab set; shares between 1 and 10 000; the active tab a tab; the focus a surface that is showing; at most 6 levels and 16 surfaces. They are properties of the type, tested over thousands of generated layouts.
- **Geometry** (`solve`): shares are proportions of what is left after one-cell dividers; every cell of the terminal belongs to exactly one tile, divider or tab strip; no tile is smaller than a minimum (20x5) unless the whole terminal is too small, in which case the **focused surface fills it alone** and the rest are hidden until the terminal grows back (nothing was changed). Deterministic. Written first against the properties above, which found two bugs in the first version (a small share could fall below the minimum while the sum fitted; swapping could move the focused surface into a hidden tab).
- **Focus** moves by direction (the nearest tile on that side sharing an edge, the longest shared edge winning) and in reading order, wrapping.
- **Edits** return a new layout or say why not: split (a new sibling in a split that already runs the same way, else a new level), add a tab, remove (closes up; the keyboard goes to the surface just before it), swap, resize (a nudge moves share between neighbours; neither goes below 1), focus on (showing its tab), and `retain` (fit to the surfaces that exist).
- **Saved form** (`saved`): versioned TOML naming surfaces by id and nothing else, at most 16 KiB, read with suspicion (valid ids, the layout invariants, bounded depth and width as the tree is read). Reading fits it to the surfaces that exist: a surface that is gone is dropped and the tree closes up; none left is no layout; a surface not named is not forced in; a stale focus falls to the first showing surface.

## Semantics to hold when the session uses it

- **One attachment per showing surface; none for the rest.** The set of attachments is exactly `layout.visible()`. Switching a tab or squeezing the terminal opens and lets go of attachments through the existing session machinery (make before break, one at a time per surface, bounded input). Resource use is bounded by 16 surfaces and, in practice, by what fits on a screen.
- **Resize is per region.** Each tile has a size; its attachment is told that size (the existing resize path, remembered across a reattach). A surface that is not showing is not resized; it takes the size of the next attachment. A terminal resize re-solves the layout and sends each changed tile its size.
- **Input has one owner: the focused tile.** The keyboard log of ADR-009 is unchanged; a focus change is a `Switch` in it, so bytes typed before the change go to the previous owner and after it to the new one, whether or not its attachment is up. Local controls stay behind `Ctrl-Space`.
- **Modes follow focus.** The inner program's terminal modes (bracketed paste, application cursor keys, mouse) are those of the focused screen model; the real terminal is switched to match when focus changes. Mouse input is translated to the tile under it.
- **The cursor** is drawn at the focused tile's cursor position only.
- **Persistence.** A layout is the user's preference for how to look, not part of a workspace definition (ADR-008): it is kept by the UI in a section of its own keyed by the workspace's identity, never in `workspaces.toml`, and never exported with a definition.

## Order

1. `flight-present`: the model, geometry, focus, edits, saved form, property tests. (this change)
2. The compositor: `vt100` adapter, region drawing, cursor, a fidelity test of the adapter against tmux's own capture, a bound on the cost of a screen model.
3. The multi-surface session: one attachment per visible tile, focus as the keyboard owner, per-tile resize, `Ctrl-Space` controls for split, close, tabs and moving focus, and the saved layout.

## Limits

- A surface appears once per layout. Two views of the same surface are not offered (they would be two attachments to one window, which tmux can do but the session's one-at-a-time rule forbids).
- No scrollback in the compositor: scrolling back is tmux's, inside the surface.
- Surfaces are windows; panes of one window cannot be shown independently (they share an active pane).
