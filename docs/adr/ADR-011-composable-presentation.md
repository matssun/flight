<!-- SPDX-License-Identifier: MIT -->

# ADR-011: Composable presentation over persistent surfaces

Status: model (`flight-present`) and compositor (`ScreenModel`, `paint`) implemented; the multi-surface session follows (see "Order"). Builds on ADR-007 (surfaces), ADR-009 (attachments, views, the surface session).

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
2. The compositor: `vt100` adapter, region drawing, cursor, a fidelity test of the adapter against tmux's own capture, a bound on the cost of a screen model. (done, below)
3. The multi-surface session: one attachment per visible tile, focus as the keyboard owner, per-tile resize, `Ctrl-Space` controls for split, close, tabs and moving focus, and the saved layout.

## Limits

- A surface appears once per layout. Two views of the same surface are not offered (they would be two attachments to one window, which tmux can do but the session's one-at-a-time rule forbids).
- No scrollback in the compositor: scrolling back is tmux's, inside the surface.
- Surfaces are windows; panes of one window cannot be shown independently (they share an active pane).

## The compositor (`flight-client::screens`)

`ScreenModel` wraps `vt100::Parser` (no scrollback); `paint` draws a solved layout into a ratatui `Buffer`: each tile's cells with their colours and attributes, wide characters kept whole (and not drawn if they would spill into the next tile), one-cell lines between tiles (the ones next to the focused tile marked), tab strips, and the focused screen's cursor for the caller to place. It is a few hundred lines; the terminal is parsed by `vt100` and diffed to the real terminal by ratatui, neither of which is ours.

**Findings that shaped it.**

- **The version is 0.16.2 (the latest).** It was 0.15.2 while ratatui 0.29 pinned `unicode-width` 0.2.0 exactly, which 0.16 cannot build with; ratatui 0.30.2 (PR #40) removed that, and the move to 0.16.2 (PR for #38) needed two changes: a cell's `contents()` is now `&str`, and the window title moved to a `Callbacks` hook (the model never used it, so `title()` is gone). The same fidelity checks pass.
- **The parser panics on two things, and the model keeps both from reaching the dashboard.** Fed random bytes (200 runs of 120 KB at each of ten sizes, escape bytes over-represented) it never panicked on a screen of at least 2x2, but it panicked on every run at any screen of one row (1x1, 2x1, 80x1), and **resizing a populated screen panicked in about 80% of runs** (in both 0.15.2 and 0.16.2, in release builds too). So: the model never gives the parser fewer than 2x2; it never resizes a parser (a resize starts a new blank screen and keeps showing the old picture until the surface, which repaints everything when its size changes, writes the first bytes of the new one: tested with a real tmux client resized from 100x30 to 60x20); and `feed` is wrapped in `catch_unwind`, so a parser failure that is neither of those empties that one surface's picture and asks for a redraw instead of ending the dashboard. What a program writes is untrusted input to a parser that is not ours.
- **Fidelity.** Against a real tmux client: rows equal `tmux capture-pane` for text, double-width, combining characters, box drawing, a wrapped line and 40 lines of scrolling; bold, reverse, underline, italic and colours reach the cells; after a resize the repainted picture equals tmux's capture at the new size (`flight-client/tests/screen_fidelity.rs`).
- **Cost.** One model holds both the normal and the alternate screen: about 150 KiB at 80x24, 870 KiB at 200x60, 2.9 MB at 400x100 when both are completely full of attributed text (16 models: 2.3, 14, 46 MB). With at most 16 showing surfaces per layout this is bounded; a hidden surface has no model.

## The presentation session (`flight-client::presentation`)

`PresentationSession` is to a layout what `SurfaceSession` is to one surface, and keeps the same promises (ADR-009) for every tile at once.

- **One attachment per showing surface, none for the rest.** Reconciling a layout attaches what has a tile and lets go of what has not (a hidden tab, a closed tile). Moving, resizing, splitting or tabbing never changes a surface or its process; letting a tile go sends `Close` and stops reading, and the surface keeps running in its backend.
- **Each attachment hears its own tile's size**, including after the terminal is resized (the resize goes to every attachment whose tile changed, ordered before further input to it).
- **Input is one ordered log**, each run tagged with the surface that had the keyboard when it was typed. It is delivered strictly in order; a focus change cannot reorder it; bytes for a surface that is still attaching or reconnecting wait (bounded: at 64 KiB the keyboard is not read, so the terminal buffers); bytes for a surface that went away are counted in `undelivered`. A surface that does not take input for 3 s has its waiting bytes given up, counted and reported, so one stuck tile cannot hold the keyboard for the others.
- **A surface is attached once at a time:** a new attachment waits (3 s at most) for the old one's `retired` signal, so keys cannot overtake keys.
- **Failure is per tile:** an exit or a lost stream is written into that tile ("the tmux client ended", "connection lost"), a broken stream is attached again to the same process [250 ms, 1 s, 3 s], and the session ends only when no showing surface can recover, or the user leaves, or the link is gone (lease renewal fails).
- **Painting:** at most one frame per 16 ms while output arrives; the picture is diffed against the previous one so only changed cells are written; the focused surface's cursor and its bracketed-paste and application-cursor-key modes are applied to the real terminal, so keys mean what that program expects. Mouse reporting is not forwarded to tiles yet (the terminal's mouse mode stays off): a known limitation.
- **Keys** (`Ctrl-Space` then): `|` or `%` split side by side, `-` or `"` split stacked, `t` new tab, `x` close tile, `n`/`p` step tabs, `h j k l` focus by direction, `o` next tile, `< > + _` resize, `a`/`s` show the agent or shell here, `q` leave, `Ctrl-Space` a literal one.
- The layout the user left is returned in the outcome.

Layout edits added for it in `flight-present`: `replace` (same place, share and keyboard) and `step_tab`.

## Entering it, and remembering it (8d)

- **Entry:** `Ctrl-Space v` inside a terminal session ends that session (after the bytes typed before it are delivered) and starts a presentation of the workspace's agent and shell side by side, the keyboard on the surface the user was in. Leaving (`Ctrl-Space q`) returns to the dashboard on that workspace. Nothing about a surface changes: its attachment is let go and opened again, the same cost as a switch.
- **Remembering:** the arrangement the user leaves is kept by the UI in `<ui config>/ui/layouts.toml` (mode 0600, atomic, at most 64 workspaces, each layout at most 16 KiB), keyed by host and workspace. It is a UI preference, not part of a saved workspace definition: it is not exported, and naming a surface in it creates nothing. On the next presentation it is read back and cut down to the surfaces that exist now; a file this build cannot read, or a newer one, is left alone and the default arrangement is used with a notice.
- **Verified against the real stack** (`flight/tests/workspace_shell_e2e.rs`, real dashboard on a pty, orchestrator, node, private tmux server): two tmux clients each about half the width; keys typed go to the focused surface only and follow `Ctrl-Space h`; closing the focused tile lets only that client go (the agent window keeps running, the shell grows to the full width); leaving saves the arrangement.

**Re-measured on vt100 0.16.2 (for #38).** A separate probe crate, 0.16.2 pinned: a one-row screen now works at 1x10 but 1x1 still panics, so the 2x2 minimum stays. Resizing a populated screen no longer panics in the shapes tried (24x80, 10x40, 5x5, 2x2 to sizes between 2x2 and 40x120) but resizing to 1x1 does, and 3 of 300 runs of random bytes with random resizes (never below 2x2) still panicked. So all three defences stay: the 2x2 minimum, no in-place resize, and `catch_unwind` around `feed`.
