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

## Decision (increment 6, part 1): every terminal attaches to a view of its own

**Finding.** The terminal path attached its tmux client to the pane's *session*. A session has one current window, so every client attached to it shows the same window: opening the second terminal onto the other surface of a workspace moved the first, and keys typed into either went to whichever window was current when tmux handled them. That is the opposite of "input is never delivered to the wrong surface", and it makes simultaneous presentation (option A and increment 8) impossible at the backend, whatever the transport does. It was invisible until now only because a switch ended the old terminal before the new one began.

**Decision.** The node attaches each terminal to a **view session**: `new-session -d -t <pane> -s flight-view-<terminal id>`, then the window and pane asked for are selected inside that view, then the client attaches to it, and `destroy-unattached` is set once it is attached. Sessions of a group share their windows (the surface is the same one) and have a current window each. The pid guard and everything after it are still one tmux command queue: a wrong pid exits 1 and creates nothing (tested). Verified on tmux 3.7b, also in `flight-node/tests/terminal_live.rs`: two terminals show agent and shell of one workspace, keys reach only the window each shows, each terminal has its own size, and releasing one removes its view and nothing else.

**What this costs, and how it is contained.**

- `list-panes -a` lists a pane once per session of its group. `parse_panes_output` folds the views: each pane comes out once, as its workspace's own session lists it, and a client looking at it through a view counts toward `focused` (and `session_attached`), so a pane shown in a Flight terminal is focused exactly as before. Every consumer (sequential and control-mode observers, reveal, open) goes through that function.
- A view is named `flight-view-` plus hex digits of the terminal id. The prefix is reserved: `valid_session_name` rejects it, so no session Flight creates for a person can be taken for a view. The node's control connection attaches to the first session that is not a view (a control client attached to a view would keep it alive).
- Cleanup does not rely on one mechanism: `destroy-unattached` removes the view with its client; the terminal's cleanup (`kill-session`) covers a client that never attached.
- A person who detaches inside the terminal (`prefix d`) detaches that view's client: the terminal ends, the workspace's session and everything in it keep running, as before.
- Terminal ids are minted by the orchestrator (128 random bits), so views never collide; the view name uses 64 of those bits.

The wire protocol did not change.

## Decision (increment 6, part 2): a surface session over a link that outlives it

**Three lifetimes, kept apart.**

| | What it is | Lives |
|---|---|---|
| Surface | a tmux window of the workspace's session, with its processes and state | in the backend; nothing in presentation code owns or ends one |
| Link | the process's one authenticated connection to the orchestrator, its runtime, its image of the fleet | for the process (reconnects by itself); `flight ui run` holds it across dashboard runs, terminal sessions and switches |
| Attachment | one terminal stream, the node's tmux client on a PTY, and its view session (previous section) | opened when a surface is shown, retired when it is not; hidden surfaces hold none |

Presentation (which surface is on screen) is a decision of the session; it never decides a surface's identity or lifetime. The session knows nothing about how a terminal is carried (`SurfaceHost`: open, connect, renew), so an in-memory host tests it and the real one (`LinkHost`) rides the link.

**Rejected, with the evidence.** *Warm (parked) attachments for the other surface* (option A): possible only since views exist, and the cold attach is 20 to 30 ms on loopback, roughly 120 ms at 20 ms RTT and 285 ms at 50 ms RTT before connection reuse; a parked attachment costs a tmux client, a view, a PTY and two streams per hidden surface, and either bandwidth for output nobody sees or a new protocol signal with a capability gate. Not needed for correctness; revisit only if measurements on a real WAN say the cold attach is the problem. *One client that selects the window* (option B): couples the surface to the attachment and cannot show two at once. *Pre-open on intent* (option C): same cost as A with a guess added.

**Input is an ordered log split at switch points.** The escape filter turns the keyboard into `Data`, `Switch`, `Leave` and `Hint` events in the order typed. Bytes typed after `Ctrl-Space s` belong to the shell from the moment they are typed, whether or not the shell is attached yet; they wait. They are delivered to that surface or reported, never to another.

| Concern | Rule |
|---|---|
| Ownership | the surface chosen by the last `Switch` that reached the front of the queue; data ahead of a switch is delivered to the previous surface first |
| Buffer | 64 KiB of queued data (switches and hints weigh a byte); a single read can overshoot by its own length (4 KiB) |
| Backpressure | at the bound the keyboard is no longer read, so the terminal's own input buffer holds the rest; nothing is dropped |
| Stall | if input cannot move for 3 s the session ends with "the surface is not taking input"; leaving (`Ctrl-Space q`) works at any time the keyboard is being read |
| Switch | make before break: the new surface is attached before the old one is let go; if it cannot be, the user stays where they were with a notice, and what was typed for the missing surface is discarded and counted (`undelivered`), not delivered to the wrong one |
| One at a time per surface | a second attachment to a surface is not asked for until the previous one has finished (its stream ended, the node read the goodbye), up to 3 s; otherwise keys on the two could overtake each other and the orchestrator would replace the one still finishing |
| Reconnect | a broken stream is re-attached 3 times (250 ms, 1 s, 3 s) to the same process only (the pid guard); a changed process is refused and the queued input is reported, not replayed |
| Cancellation | a switch requested while another is opening cancels the one in flight; a session that ends cancels everything; a terminal whose `OpenTerminal` was answered but never attached ends by its own attach window (bounded, existing) |
| Resize | the size is the terminal's, remembered by the session: an attachment opened at one size is told the current one as soon as it is up, and a resize while a surface is opening is not lost. A surface that is not attached has no client, so nothing constrains it; it takes the size of the next client that attaches. Each view has its own size, so two attached surfaces can differ |
| Limits | per UI 2 terminals that are not closing; a closing terminal still counts toward the node's 4 and the total 32. Multiple simultaneously visible surfaces raise the per-UI figure (a policy, not a design change) |
| Lease | renewed over the link for the surface on screen; it fails the session only when the link has stopped |
| Authorization | unchanged: every terminal is asked for from the orchestrator with the pid guard; the identity checks, single-use ids and lease are the orchestrator's |

**Teardown is ordered, and four bugs of ordering were found by driving the real dashboard with keys typed in bursts across switches** (`keys_typed_right_after_opening_or_switching_reach_the_surface_they_were_typed_for`, which failed 1 time in 3 until all four were fixed and has since passed 30 of 30 on a release build and 10 of 10 on a debug build):

1. The UI dropped a terminal's stream as soon as the session let go, which cancels it with the goodbye and the last keys still unsent. The stream is now read to its end (2 s bound).
2. The orchestrator tore the node's side down when the UI said goodbye, discarding what was queued toward the node. It now keeps the node's side open until the node has ended it (3 s bound) and marks the terminal *closing*: not replaced by a new open of the same pane, not counted against the UI's limit, gone after 5 s whatever happens.
3. The node gave up reading input when a send failed, and read input only between floods: under a program that floods the terminal a Ctrl-C waited for the flood (3 s with the default stall limit, measured). It now reads input between output frames, drains for 2 s after the orchestrator stops taking output, and leaves the tmux client alone for 100 ms after a goodbye so that the key just written is read by it.
4. The dashboard left keys in the terminal library's event queue when it let the keyboard go (lost in debug builds, where it is slower). Keys typed after opening a surface are now read in order, encoded as a terminal sends them, and handed to the session; raw mode stays on across the hand-over, and what is typed for a surface that does not open is discarded and counted in the dashboard's message.

**Measured after** (same harness, release build, 15 switches; the dashboard is no longer torn down, the link is reused, the lease rides the link, the terminal's threads stop at once, and the reveal still happens only for the dashboard's first open):

| simulated RTT | before p50 (min-max) | after p50 (min-max) |
|---|---|---|
| 0 ms | 407 (406-412) | 30 (24-40) |
| 20 ms | 615 (595-635) | 117 (107-122) |
| 50 ms | 1001 (947-1166) | 262 (249-272) |

What remains at 50 ms RTT is the routed `OpenTerminal` (about 2 RTT) and a stream dial (about 2.3 RTT) per switch; the next step is to reuse the terminal connection for new streams. Not done: removing the redundant `RevealPane` from the dashboard's first open (it also reveals when a node offers no terminals, a behaviour ADR-003 describes), and raising the per-UI terminal limit for simultaneous presentation (increment 8).

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
