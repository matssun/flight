<!-- SPDX-License-Identifier: MIT -->

# ADR-004: Terminal-session liveness

Status: Accepted with refinements; lease implemented (slices 2 to 6 below), cancellation changes not needed (see Step 0 result). Follow-up to the two liveness limitations recorded in ADR-003 ("Stall rules"). Not in scope: SSH, resume/replay, shared terminals, `SendInput`, `KillPane` guarding, UI-to-node end-to-end encryption.

## Problem

1. **Idle wedged UI.** The stall rule fires only when an output send blocks. A UI that is alive at the transport level but stopped doing anything, on a pane that produces no output, is indistinguishable from a legitimately idle terminal. It holds a slot (2 per UI identity, 4 per node) until its connection disappears.
2. **Node loss behind a blocked relay.** With the orchestrator pump blocked sending to a wedged UI, loss of the node was not observed promptly in measurement; cleanup waited for the 30 s stall deadline.

Both are one defect: **the lifetime of a terminal is inferred from data movement, and tasks that are blocked on data movement cannot see anything else.**

## What the existing code gives us (read before deciding)

- A terminal is its own gRPC stream on its own HTTP/2 connection, per side. HTTP/2 PING keepalive (10 s interval, 10 s timeout, both ends) is answered by the HTTP/2 layer of a live process. It proves "the process and its socket are alive", **not** "the UI application is making progress". So keepalive cannot distinguish a wedged UI; an application-level signal is required.
- `terminal_stream::pump` (orchestrator) has an outer `select!` over `inbound.message()`, `abort` and `gone`, but once it holds a frame and is blocked in `forward.send`, the inner `select!` watches only `abort` and `gone`. It does **not** poll `inbound`, so anything arriving on that side (including a heartbeat, EOF or reset) is unseen while it is blocked. It is the structure, not the timeout, that makes it slow to notice.
- The node's `run` loop has the same shape: while its output branch is inside `client.send(..)` under `timeout(stall)`, it does not poll `client.next()` or any cancellation.
- `abort` (a `watch`) is already the cancel signal for a pump, and `Core::node_disconnected` / `end_conn_terminals` already turn loss of a node's control connection into `terminals_ended` -> `relay.abort`. `ui_disconnected` ends nothing; only revocation ends a UI's terminals.
- The core is clock-driven by `now` and `tick`, with `expire_terminals` already there for attach deadlines.

## Step 0 result and refinements (after review)

- **Step 0: the node-loss case does not reproduce.** `a_node_lost_while_the_relay_is_blocked_on_a_wedged_ui_is_noticed_promptly` (`flight-transport/tests/terminal.rs`) runs a flooding pane, a UI that stops reading, waits until the orchestrator's queue toward the UI is full and 8 s more (HTTP/2 windows full), then makes the node vanish with no goodbye (its runtime and sockets are dropped). The orchestrator notices in about 50 ms, with stall limits set to 60 s so only the loss signals can fire. No signal path is missing for an abrupt loss, so **the pump and node loops were not restructured**; part 2 below stays as a rule for future code, not a change. The test reports whether the orchestrator's queue toward the UI was actually full before the node vanished: it was on the development machine (macOS); on GitHub's Ubuntu runners the queue did not fill within the wait (HTTP/2 windows absorb more there), so on that platform the result shows prompt loss detection with the UI not reading, not proof of a full queue. Not covered by that test: a silent partition (a stopped process, no FIN), which is bounded by HTTP/2 keepalive (about 20 s) and not yet measured; and a real process kill on each supported platform. The earlier ADR-003 measurement is likely explained by the first implementation, before the `gone` signal existed.
- **`terminal_v1` carries the lease.** No separate capability.
- **Where the lease rides.** The dashboard's `UiConnect` stream cannot carry it: `flight ui run` drops the dashboard (and its backend and stream) before the terminal is presented. The presenter therefore opens a `UiConnect` of its own that subscribes to nothing and sends leases. Same RPC and control path; no new stream type.
- **Who generates it.** The lease is renewed from the presenter's relay loop (`relay`, outer `select!`), not from a generic timer or the process: if the relay is not being polled, no lease is sent. A failure to send (control connection gone) ends the presentation. A healthy unrelated UI task cannot renew it.
- **Timing figures are targets**, to be promoted to promises only after the tests prove them on each supported platform. The lease path (15 s) is asserted in tests with a 3 s TTL scaled accordingly.

## Decision

Three small parts. Each is needed; none is a timeout change.

### 1. A terminal lease, renewed by the UI on its control stream

- New UI request `TerminalLease { terminal_id }` (additive field in `UiRequest`'s oneof) sent on the UI's existing `UiConnect` stream, **not** on the terminal stream.
- **Why the control stream.** The orchestrator reads `UiConnect` continuously regardless of terminal output; the terminal stream's reader can be blocked on backpressure (see above) and a heartbeat queued behind terminal data inherits that blocking. Separate streams are also how ADR-003 already keeps a stalled terminal from touching control traffic; a 20-byte lease is not terminal traffic.
- **Orchestrator authority.** `Terminal` gains `lease_until`. Set to `now + LEASE_TTL` when both ends have attached; renewed to `now + LEASE_TTL` on each valid lease. A lease is valid only if the UI connection's authenticated identity equals `terminal.ui_identity` and the id is live; anything else is ignored and indistinguishable from an unknown id (same rule as every other refusal). Expiry uses only the orchestrator's own monotonic clock; the UI sends no timestamp (no skew).
- **Expiry** is in `expire_terminals` on the existing tick: `lease_until <= now` -> remove from table, push `terminals_ended` with new `ExitReasonCode::LEASE_EXPIRED`, which already reaches `relay.abort` and therefore every pump and the node's stream. No new teardown path.
- Orchestrator pauses do not count against the UI: if the gap between two ticks exceeds `LEASE_TTL / 2`, every lease is extended by the gap (the orchestrator was not running, so it cannot claim the UI was silent).
- The UI's presenter sends a lease immediately on `TerminalOpened` and then every `LEASE_PERIOD` from a timer task that lives exactly as long as the presenter. It is independent of output, input and the terminal stream. Leaving the terminal (detach key, end, error) stops it; the lease then lapses by itself even if the explicit `Close` is lost.

### 2. Nothing in a terminal task may await without a cancel arm

Rule, enforced by structure and review: every `await` in the orchestrator pump and the node `run` loop is raced against the terminal's cancel signal and its peer-gone signal.

- **Orchestrator.** Make the blocked-send arm complete: `abort` and `gone` are already there; add the other side's end. A pump whose own side ended calls `finish_terminal`, which aborts the relay, which cancels the other pump's blocked send. Concretely `finish_terminal` stays the single exit, and a pump blocked in `send` is always cancellable through `abort`. **Step 0 of implementation is a failing test that reproduces the measured case (node killed while the UI-side pump is blocked on a wedged UI) and identifies which signal failed to fire** (control-connection loss not reaching `end_conn_terminals`, or the response stream not dropping, or the abort not reaching the other pump). The fix is then the smallest change at that point; this ADR does not guess the cause.
- **Node.** Split output sending from the main loop: a sender task owns `client.send` for output (one frame in flight, as today), and the main loop keeps polling `client.next()`, input, resize and a cancel token. Stream end, `Close`, an `Exit` from the orchestrator or cancellation drops the sender task (its blocked send is cancelled) and proceeds to the existing hang-up/reap. The input `cmd_tx.send(..).await` is raced against the same token.
- The cancel token for the node is also fired when the node's control link to the orchestrator ends, so a terminal does not outlive the node's session with the fleet.

### 3. Unchanged

Output stays bounded and lossy under sustained backpressure (64 KiB buffer, 16 KiB frames, repaint on recovery); input stays lossless; the 30 s stall rule stays as the backstop for "alive, heartbeating, but not reading output" (the lease proves the UI process is turning, the stall rule proves it is draining). The "tmux client exited" rule from PR #4 stays.

## Constants

| Constant | Value | Rationale |
|---|---|---|
| `LEASE_PERIOD` | 5 s | Cheap (one tiny frame per terminal per 5 s), well under TTL |
| `LEASE_TTL` | 15 s (3 periods) | Tolerates two lost or late leases |
| stall limit | 30 s (unchanged) | Output-drain backstop |

Lease frames are one per terminal, at most 2 terminals per UI identity.

## Answers

1. **Origin and cadence.** The UI presenter, every 5 s, plus one at open, on the UI control stream. Orchestrator is the authority; the node does not hold a lease and ends when told or when its own connections end.
2. **Expiry condition.** `now >= lease_until` on the orchestrator's tick, with `lease_until = last valid lease received + 15 s` (plus any orchestrator-pause extension). Not output, not input, not terminal-stream state.
3. **Liveness vs flow control.** Yes, by construction: it is on a different stream and is consumed by a loop that no terminal data can block. Terminal-stream backpressure (HTTP/2 window, 4-frame queues) never touches it.
4. **Cancelling a blocked send.** A send is always one arm of a `select!` with the terminal's `abort` watch and peer-gone signal. Loss of a node (control connection ends, revocation, orchestrator shutdown), lease expiry, UI-side end or revocation all go through `terminals_ended` or `finish_terminal`, which sets `abort`; both pumps wake at once. On the node, the cancel token and the stream-end arm cancel the sender task.
5. **Delay and jitter.** Two missed leases are free (TTL = 3 periods). A late lease still renews from its arrival time. Timer: `interval` with `MissedTickBehavior::Delay` (no burst after a stall). Orchestrator pauses extend leases by the pause. A UI process that is suspended or whose machine sleeps for more than 15 s loses its terminal; on resume it sees `LEASE_EXPIRED` and returns to the dashboard. This is intended: a terminal is for an active UI, with no resume.
6. **Idle is not wedged.** Expiry reads only the lease, never output or input age. An idle terminal with a live UI renews forever; this is a named test.
7. **Cleanup proof.** See tests.
8. **Promised bounds** (see the table below).

## Timing bounds Flight promises

| Event | Terminal is gone, all resources released, within |
|---|---|
| UI process killed / connection closed cleanly | 2 s (stream end is seen; the lease is the fallback at 15 s + one tick) |
| UI wedged or suspended, pane idle or busy | 15 s + one tick (lease), + 2 s reap |
| UI reads nothing, pane busy | 30 s (stall rule, unchanged) |
| Node process killed, UI healthy or wedged, relay blocked or not | 2 s on a connection reset; for a silent partition, the control link's HTTP/2 keepalive (about 20 s). Never the stall limit. |
| Revocation, orchestrator shutdown | 2 s |

"Within" is measured from the cause to every resource below being released. The numbers are tested bounds with CI headroom, not real-time guarantees; the first four rows are asserted at their stated values in tests with injected clocks where the cause is a lease, and with real sockets where the cause is a reset.

## Tests

A single helper, `assert_terminal_gone(rig, id)`, polls (deadline = the bound being promised) until **all** hold, and fails naming the one that does not:

- the PTY child and the tmux client are gone (`list-clients` on the private `-L flight-test-<pid>` socket shows none; the child pid is reaped);
- the node's writer and reader threads are joined (a drop-guard counter, `terminal_threads_live() == 0`);
- the node's terminal slot is released (semaphore permits restored);
- the orchestrator's table entry is gone (`terminal_count()`, `terminal_is_open(id)`), the `relays` entry is gone and both of its queues report closed;
- no relay task is alive (a drop-guard gauge on pump tasks, `terminal_tasks_live() == 0`);
- the id is dead: attach with it is refused.

Scenarios, each ending in the helper:

1. **Core, in-memory clock:** lease renewed keeps alive for 10 TTLs with no output (idle is not wedged); no lease for TTL expires with `LEASE_EXPIRED`; leases at TTL-1 s survive; late-by-two-periods survives; foreign identity lease ignored; unknown id ignored; orchestrator pause (tick gap > TTL/2) extends rather than expires.
2. **Idle wedged UI** (real streams): UI stops leasing and reading, pane silent -> gone within bound.
3. **Busy wedged UI** with a flooding pane (the existing wedge test): lease still expires first, or stall at 30 s if leases continue and nothing is read (two variants).
4. **Node killed while the orchestrator pump is blocked on a wedged UI:** the reproduction from Step 0, asserted at the 2 s reset bound, and a variant with a silent partition at the keepalive bound.
5. **UI killed** mid-session, pane idle and busy.
6. **Revocation** of the UI and of the node; orchestrator shutdown.
7. **Idle terminal lives** across 3x the TTL with a healthy UI and typing still works afterwards.
8. **Repeat:** open/kill 50 terminals; `terminal_count`, thread and task gauges, tmux client count return to zero and process memory is flat (guards against slow leaks, which is what prompted this work).

## Compatibility

`TerminalLease` and `LEASE_EXPIRED` are additive protocol changes with golden-hex and schema-drift updates. Assumption to confirm at review: `terminal_v1` (merged in PR #4) has not been released, so the lease is part of `terminal_v1` rather than a new capability. If it has shipped, the lease becomes `terminal_lease_v1`, a UI whose terminal lacks it keeps today's behaviour, and the orchestrator enforces expiry only for terminals whose UI declared it.

## Open points for review

1. The UI must keep its `UiConnect` stream while presenting a terminal (the dashboard is suspended). To verify in `flight-client`; if the state stream is dropped then, the presenter must hold it, or the lease needs a small dedicated stream.
2. Heartbeat meaning: it proves the UI's async runtime and control connection are turning, not that its terminal emulator is painting. A paused user terminal (Ctrl-S) on an idle pane stays alive by design; on a busy pane the stall rule ends it. Say so if you want a stricter definition.
3. Step 0 may show the node-loss cause is outside the pump (for example control-link loss detection); the cancellation rule in part 2 still applies, and the change stays confined to wherever the failing test points.

## As built

- Protocol: `TerminalLease` in `UiRequest` (tag 3), `ExitReasonCode::LeaseExpired = 8`.
- Core: `Terminal.deadline` doubles as the lease deadline once both ends are attached (`terminal_lease_ttl_secs`, default 15); `terminal_lease` renews only for the owning identity and a fully attached terminal; `forgive_pause` extends leases by a tick gap longer than TTL/2; `expire_terminals` reports `LeaseExpired` for attached terminals and `StartFailed` otherwise.
- Presenter: `flight-client` `Lease` (period 5 s), called from `relay`.
- Tests: core table (idle never expires, expires with its own reason, late renewals survive, foreign identity and unknown ids ignored, lease does not extend the attach window, orchestrator pause forgiven); transport with real streams and tmux (idle terminal outlives four lifetimes and still works, no lease on an idle pane, no lease and no reading on a flooding pane with stall limits at 60 s, node vanishes behind a blocked relay, 50 open/close/expire cycles past the node's limit of four with tmux clients, table entries and relays back to zero each time).
- Presenter control failure (`flight-client/tests/terminal_session.rs`, `a_presenter_whose_control_connection_dies_stops_renewing_and_the_terminal_goes`): a healthy terminal with traffic and a lease renewed every second; the presenter's dedicated control connection is then dropped with the terminal stream untouched. Observed: the next renewal fails, **the presenter itself ends the presentation** (`TerminalEnd::Lost("... control connection ...")`) about 1.0 s later (one renewal period) and drops its terminal stream, so the terminal ends through the ordinary stream-end path, not through the orchestrator's lease expiry and not through the 30 s stall rule. Renewals stop at that point (none in the following 2 s); the tmux client, table entry and relay are gone; five further terminals then open and close, past the node's limit of four, so no slot was consumed. The orchestrator-side lease expiry (15 s) is therefore the backstop for a presenter that is wedged and cannot even end itself; that path is covered by the transport tests that stop renewing while keeping the streams open.
- Not built: drop-guard gauges (not wanted unless later adopted as operational metrics) or a stopped-process (silent partition) test; silent-partition timing, the 2 s hard-disconnect figure and per-platform real-process-kill timing remain follow-up measurements.

## Slices

1. This note.
2. Step 0: failing test for node loss behind a blocked relay; root cause recorded here.
3. Protocol + core: lease frame, `lease_until`, expiry, pause extension, `LEASE_EXPIRED`; in-memory-clock tests.
4. Transport: lease handling on `UiConnect`; the cancellation fix from step 0; drop-guard gauges and `assert_terminal_gone`.
5. Node: sender task split, cancel token tied to the control link.
6. UI: lease timer in the presenter; return to dashboard with the reason on `LEASE_EXPIRED`.
7. Scenario and repeat tests; update ADR-003's "Stall rules" paragraph to point here.
