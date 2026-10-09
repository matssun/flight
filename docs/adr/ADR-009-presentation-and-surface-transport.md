<!-- SPDX-License-Identifier: MIT -->

# ADR-009: Presentation model and persistent surface transport

Status: investigation and direction. The baseline measurements below were taken for increment 6; the design that follows from them is recorded under "Decision (increment 6)" as it lands. The choices that are necessary are separated from those that can wait.

## Two problems, solved separately

1. **Surface transport**: reaching a surface without rebuilding a connection each time. Today a switch is: leave surface A (the node's PTY client and the orchestrator terminal lease end), return to the dashboard, `RevealPane`, `OpenTerminal` (a new PTY running `tmux attach`, a new single-use lease and streams), then bytes flow. That is the roughly 0.25 to 0.45 s of ADR-007, and keystrokes typed before the new stream is up have nowhere to go.
2. **Presentation**: how several surfaces are arranged on one screen.

They are independent: (1) is useful with a single visible surface (instant switching, no lost keys) and is a prerequisite of (2), not the reverse.

## Necessities

- **Presentation is separate from surface lifetime.** A layout refers to surfaces by `SurfaceId`; hiding, switching or resizing never ends a surface. This is already true of the backend (a surface is a tmux window); the invariant to keep is that no presentation code owns or closes one.
- **A layout is a tree**, so focused views, tabs, splits and nesting are all one shape: `Region = Surface(SurfaceId) | Split { axis, ratios, children } | Tabs { active, children }`. Persisting it is optional; if it is, it is a separate section keyed by `ConfigKey`, never part of the workspace definition.
- **Keystrokes must not be lost.** Input typed while a transport is being established is either buffered and delivered in order, or the client does not accept it (cursor stays on the dashboard) until the stream is up. Dropping silently is not acceptable.

## Evidence: the current switch, measured (increment 6 baseline)

Harness: `flight/tests/surface_switch_measure.rs` (the real binaries, the dashboard in a pty, `Ctrl-Space a|s` timed to the other surface's status marker on screen) and `flight/tests/surface_switch_phases.rs` (the library path the dashboard takes, phase by phase). Both are `#[ignore]`; `FLIGHT_MEASURE_RTT_MS=n` puts a delaying TCP proxy (`delay_proxy` in `flight/tests/support`) between the UI and node and the orchestrator, so one UI-to-orchestrator round trip costs `n` ms on top of loopback. The proxy never looks at the bytes (the connection stays mutual TLS end to end). macOS, release build, tmux 3.7b, 15 switches per run (6 with latency).

**Whole switch, key to new surface on screen** (spread across runs under 3 ms at 0 ms):

| simulated RTT | min | p50 | max |
|---|---|---|---|
| 0 ms (loopback) | 406 | 407 | 412 |
| 20 ms | 595 | 615 | 635 |
| 50 ms | 947 | 1001 | 1166 |

**Where the time goes**, from timestamp probes placed in the real binaries for this investigation (not kept in the tree), loopback, one switch:

| t (ms) | event |
|---|---|
| 0 | escape recognized in the relay |
| 0.8 | goodbye done; the node's tmux client is hung up within 13 ms |
| 101 | the dashboard starts again: the terminal's stdin and size threads wake only every 100 ms, and the terminal waits for both to join |
| 303 | the dashboard asks to switch: a new backend, a new TLS connection and subscription, the first snapshot, and the 100 ms event poll that turns the snapshot into the pending switch |
| 320 | `RevealPane` and `OpenTerminal` answered (17 ms); the node's tmux client is already painting |
| 406 | the dashboard has exited (its worker polls too), a new runtime is built, the terminal stream and lease are connected, first output is on screen |

About 380 of the 407 ms are the **dashboard being torn down and rebuilt**, not the transport. The part Flight called "transport" (reveal, open, dial, spawn a tmux client, first byte) is about 35 ms on loopback.

**The transport phases under latency** (`surface_switch_phases`, p50, a cold path: new backend, new connections):

| phase | 0 ms | 20 ms | 50 ms | roughly |
|---|---|---|---|---|
| backend start + first snapshot | 3 | 73 | 174 | 3.5 RTT (TLS dial, subscribe, snapshot) |
| `RevealPane` + `OpenTerminal` | 25 | 122 | 252 | 5 RTT: two sequential routed commands, each UI-orchestrator and orchestrator-node |
| terminal stream dial (UI) | 1 | 48 | 116 | 2.3 RTT: TCP, TLS, HTTP/2, attach |
| lease connection dial (UI) | 1 | 49 | 118 | 2.3 RTT, for a second connection that only says "still here" |
| total | 34 | 294 | 657 | |

So on loopback the cost is structural (rebuilding the dashboard), and on a real network it is connection setup repeated for every switch: three fresh mutual-TLS connections (UI terminal stream, node terminal stream, lease) and two sequential routed commands, of which the first (`RevealPane`) is redundant for a terminal because the guarded `tmux_attach_command` already selects the window and pane under the same pid guard.

**Resources while one surface is shown** (loopback, one workspace with two surfaces): one tmux client (a pty and a `tmux attach` process) on the node per open terminal; node 17 threads/16 fds, orchestrator 20 threads/21 fds, dashboard 15 threads/19 fds. Hidden surfaces cost nothing today because they have no attachment at all; a switch ends the old attachment before the new one exists, so no input can be delivered to the wrong surface, and keys typed in between have no consumer.

**Behaviour of the backend that constrains the design (tmux 3.7b, checked on a private socket)**:

- The windows of a session share one *current window*. Two clients attached to the same session therefore cannot show two surfaces of one workspace at once; "warm" clients (option A above) for the other surface of the same session would just show the same window. Independent concurrent views need **session groups**: `new-session -d -t <workspace-session> -s <view>` gives a session with the same windows and its own current window.
- In a group, `list-panes -a` reports every pane once per session of the group, and session options (`@flight_session`, `@flight_workspace`) are not inherited by the view session (window options such as `@flight_surface_id` are shared, being per window). A node that publishes by session option therefore does not publish view sessions, but anything keyed by pane id alone must be checked for the duplicates before views are used. This is the node-side prerequisite of simultaneous presentation; it is invisible to the wire protocol, which names a surface and gets an attachment.

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
