<!-- SPDX-License-Identifier: MIT -->

# ADR-009: Presentation model and persistent surface transport

Status: investigation and direction. Nothing here is implemented; the choices that are necessary are separated from those that can wait.

## Two problems, solved separately

1. **Surface transport**: reaching a surface without rebuilding a connection each time. Today a switch is: leave surface A (the node's PTY client and the orchestrator terminal lease end), return to the dashboard, `RevealPane`, `OpenTerminal` (a new PTY running `tmux attach`, a new single-use lease and streams), then bytes flow. That is the roughly 0.25 to 0.45 s of ADR-007, and keystrokes typed before the new stream is up have nowhere to go.
2. **Presentation**: how several surfaces are arranged on one screen.

They are independent: (1) is useful with a single visible surface (instant switching, no lost keys) and is a prerequisite of (2), not the reverse.

## Necessities

- **Presentation is separate from surface lifetime.** A layout refers to surfaces by `SurfaceId`; hiding, switching or resizing never ends a surface. This is already true of the backend (a surface is a tmux window); the invariant to keep is that no presentation code owns or closes one.
- **A layout is a tree**, so focused views, tabs, splits and nesting are all one shape: `Region = Surface(SurfaceId) | Split { axis, ratios, children } | Tabs { active, children }`. Persisting it is optional; if it is, it is a separate section keyed by `ConfigKey`, never part of the workspace definition.
- **Keystrokes must not be lost.** Input typed while a transport is being established is either buffered and delivered in order, or the client does not accept it (cursor stays on the dashboard) until the stream is up. Dropping silently is not acceptable.

## Options for persistent transport (to measure, not yet to choose)

| Option | Idea | Cost | Risk |
|---|---|---|---|
| A. Warm the other surfaces | When a workspace is opened, the node keeps (or pre-opens) the PTY client for its other surfaces; switching changes which stream the UI renders | one tmux client per surface per open workspace; bytes of hidden surfaces must be discarded or coalesced | the existing bounded-lossy output policy needs a "hidden" mode (drop and repaint on show) |
| B. One stream, node switches the pane | One terminal stream per UI; a control message tells the node to `select-window` in the same tmux client | smallest; one client | surfaces share one tmux client's size and state; cannot show two at once, so a dead end for side by side |
| C. Pre-open on intent | Open the other surface's stream when the user opens a workspace or hovers, keep for a TTL | bounded resource use | still a rebuild when cold |

A and C extend to N surfaces and to simultaneous presentation; B does not. Recommended first experiment: **A with a hidden mode**, measured against the current path with the existing driven end-to-end test (`flight/tests/workspace_shell_e2e.rs`). The orchestrator's per-UI and per-node terminal limits (2 and 4) are the first thing it hits and must become per-surface-aware.

## Simultaneous presentation (deferred)

Composing two interactive byte streams into one terminal needs a screen model per surface (a terminal emulator) and a compositor, or delegating to tmux splits, which makes surfaces panes of one layout and gives up independent presentation (ADR-007). That decision is deliberately last: it depends on measurements from the transport work and on whether a headless emulator crate meets the repository's dependency and licensing standards. The layout tree above is the only commitment.

## Order

1. Persistence and recovery (ADR-008).
2. Persistent transport experiment (A), with keystroke-loss tests.
3. Only then, emulation/composition for side by side.
