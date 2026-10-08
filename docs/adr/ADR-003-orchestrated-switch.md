<!-- SPDX-License-Identifier: MIT -->

# ADR-003: Switching to a pane through the orchestrator

Status: Revised and approved (remote presentation is a Flight-native terminal session). Slices 2 (protocol and node-side reveal) and 3 (orchestrator routing) are implemented and stand. Slice 4 is implemented for local presentation only; its ssh-based remote presentation is **superseded** by the Flight-native terminal session below and is to be removed. Nothing in this revision is implemented yet.

## Invariant

**Normal distributed Flight operation requires no SSH configuration and no direct network path from the UI machine to a node.** All control and all interactive presentation travel over Flight's authenticated connections: UI to orchestrator, node to orchestrator (both dialled outward from the UI and the node). Nodes may be reachable only through their own outbound connection. SSH is not part of the normal UX, and no code path in the default build needs an ssh binary, an alias, or a mapping file.

## Problem

Enter on a pane in the orchestrated dashboard says "not available yet" (ADR-001 limit, ADR-002 "Not yet"). The protocol had an unguarded `SwitchPane` that no node ever offered (removed, see below). The hard part is not routing: `tmux switch-client` acts on a tmux client, and "the client" is a property of where the user's terminal is, which is not where the pane is when the pane is on another machine.

## Decision

**"Switch" is two operations with different owners.**

1. **Reveal** (server state, owned by the node): make the target the active pane of its window and its window the current window of its session. `select-window -t @W` then `select-pane -t %P` on the node's own tmux server. A node is a service with no tmux client and no way to know which client is the user's, so it never touches clients.
2. **Present** (client state): show the pane in a terminal the user is looking at. For a pane on the UI's own machine the UI points a local tmux client at it. For a pane on another machine the node runs a tmux client inside a PTY it owns, and the UI's terminal is connected to that PTY through Flight ("Remote presentation" below). The node still never touches a client the user owns; the client it starts is its own, created for this one session.

The wire command is `RevealPane`, named for what it does (an earlier draft called it `SwitchPane`, which would have suggested a client switch). The orchestrator routes the reveal (the existing control path: capability check, immediate typed failures for unknown, disconnected or incapable nodes). The dashboard's Enter composes reveal and present as two explicit stages, so a failure says which one failed ("pane selected, but the terminal could not be opened: node has too many terminals"). The orchestrator also relays the remote terminal stream, but never interprets it.

### The plan: a pure function of three facts

`plan_switch(pane, ui_context) -> SwitchPlan`, unit-testable with no tmux and no network. `ui_context` is: the node identity of this machine (below), whether the UI runs inside a tmux server and which, and nothing else: there is no per-node configuration (an earlier draft added an ssh destination map; see "Superseded").

| Case | Plan | What happens |
|---|---|---|
| Pane is on this machine and the UI runs inside the pane's tmux server | `SwitchClient` | Guard locally, then `switch-client -c <ui client> -t <pane>`, then the UI exits (as the direct dashboard does). No orchestrator round trip. |
| Pane is on this machine and the UI is not inside any tmux | `AttachLocal` | Guard and reveal locally, then replace the UI with `tmux -L <server> attach-session -t =<session>`. |
| Pane is on another machine | `RemoteTerminal` | `RevealPane` through the orchestrator (guarded by the node), then `OpenTerminal` (guarded again, atomically), then the UI relays its terminal to the node's PTY until the terminal ends, and returns to the dashboard. No ssh, no destination, no UI-to-node path. |
| Anything else | `Refuse(reason)` | Nothing is changed. The message says exactly what to do. |

`Refuse` covers, each with its own message: the UI runs inside a *different* tmux server than the pane's (nesting is the user's explicit choice, never ours); the UI's client cannot be identified; the node does not offer `guarded_reveal_v1` or `terminal_v1` (an older node, or one started with `--no-terminal`); the node is disconnected. The remote plan does not depend on where the dashboard runs (inside tmux or not): the stream is a separate PTY, not a tmux client of the UI's server.

### Which client is "the UI's client"

No "pick any attached client", and not "ask tmux which client I am". Measured against real tmux with pty-attached clients (`display-popup` on an attached client):

- Inside a popup, `$TMUX` is set and `$TMUX_PANE` is empty. In an ordinary pane both are set.
- With **one** client on the session, `display-message -p '#{client_name}'` from the popup names it correctly.
- With **two** clients on the same session, a popup opened on the idle client was told it was the *other* client (tmux answers with the most recently active one). Trusting `display-message` here would switch the wrong terminal.

So the rule is: find the UI's **session** (ordinary pane: `display-message -p -t $TMUX_PANE '#{session_name}'`, which does not depend on any client; popup: the session id in `$TMUX`), list that session's attached clients, and proceed only if there is **exactly one**; that client is the target of `switch-client -c`. Zero or several is `Refuse("cannot tell which tmux client shows this dashboard; attach manually")`, even though it occasionally makes a user attach by hand. Refusing is better than switching the wrong terminal. The slice that implements this carries live tests with pty-attached clients for the one-client, two-client and popup cases.

### Which machine is "this machine"

The node role directory under the UI's config dir (`<config-dir>/node/`, joined to the same orchestrator) gives the local node's `HostId` (its identity fingerprint, which is what `PaneRef.host` carries). `--node-dir` overrides it. No node role on this machine means there is no local plan: every pane is `AttachRemote` or `Refuse`. Matching by pane id and pid alone is not used: it would treat a coincidentally equal `%12` on another machine as local.

### Stale-pane guard

A pane id is reused across process lifetimes. Today's `KillPane` guard compares the node's *published* pid with tmux's current pid, which protects against tmux changing under the node but not against a UI that is looking at an older picture than the node.

For switch the request carries the pid the UI saw, and every step checks it:

- `PaneState` gains `pid` (tag 14, additive; it changes only when the process does, so it adds no per-poll delta; 0 from a node that predates it). The UI view model will carry it.
- `RevealPane { pane_ref, expected_pid }` is a new command (tag 6, `expected_pid` validated non-zero). The old unguarded `SwitchPane` (tag 2) was never offered by any node; it is removed and its tag retired, and a peer that still sent it is refused as a command with no kind.
- The node refuses unless the published pid, tmux's current pid for that pane, and `expected_pid` all agree, with a new `PaneChanged` failure (not `UnknownPane`) so the UI can say "the pane changed, refresh" and not "no such pane". A stale request is refused before it becomes a job.
- The local plans run the same check against tmux directly.

**The general rule:** any control action whose meaning depends on a particular pane process must carry the pane incarnation the caller observed (today `PaneRef` plus `expected_pid`).

**Capability, not just a field.** Semantic compatibility matters more than protobuf compatibility. The reveal is negotiated as `guarded_reveal_v1`; a node that does not offer it is refused by the orchestrator (`Unsupported`, nothing forwarded), and a node refuses the command if it was not accepted. A newer UI talking to an older peer gets "upgrade required", never an unguarded older behaviour.

Reveal is idempotent and harmless, so a lost response never needs compensation; the UI starts the attach only after a success.

### Remote presentation: a terminal session over Flight

```
Flight UI  <-- TerminalFrame stream -->  Orchestrator  <-- TerminalFrame stream -->  flight-node
 (raw tty)        (UI dials out)          (splices,         (node dials out)          PTY master
                                           never parses)                                  |
                                                                                 tmux attach (client)
                                                                                          |
                                                                                 target session/pane
```

#### Why a separate stream, not more frames on the existing ones

The existing UI and node streams carry fleet state and control. Terminal bytes on them would queue behind snapshots and deltas (and the other way round), would share their outbox overflow rule (overflow means resync, which is meaningless for a byte stream), and would let one stalled terminal consume the HTTP/2 connection window of the control link. So a terminal is its own gRPC stream on its **own connection**, opened by each side dialling the orchestrator (outward only, so the topology invariant holds): the node's control link and the UI's state stream are untouched by terminal traffic or its stalls. The control path carries only the *request* to open one.

#### Messages

Control path (existing request/response machinery, routing, 10 s timeout, typed failures):

- `Command.OpenTerminal { pane_ref, expected_pid, cols, rows, term, terminal_id }` (oneof tag 7). Validation: pid non-zero (as for reveal); `cols` in 1..=1000 and `rows` in 1..=1000; `term` matches `[A-Za-z0-9._-]{1,32}` (it becomes `$TERM` of the PTY and nothing else); `terminal_id` is **never chosen by a UI**: the field exists only so the orchestrator can pass its own id to the node, and a UI-sent command with it set is rejected at the boundary as `InvalidRequest`. A node that receives an `OpenTerminal` without a 16-byte id refuses it.
- Capability `terminal_v1`, required by routing for `OpenTerminal`, advertised by a node unless `--no-terminal`. Like `guarded_reveal_v1` it is negotiated, and an older peer is refused, never given an unguarded behaviour.
- `Response.terminal` (`TerminalOpened { terminal_id }`, response tag 5): the success answer, carrying the id the UI must now attach to.
- New error code `Busy` (limit reached, below); `PaneChanged`, `UnknownPane`, `Unsupported`, `NodeUnreachable` are the existing ones.

Data path: two new RPCs on the `Flight` service, both `(stream TerminalFrame) returns (stream TerminalFrame)`, both requiring the same mTLS identities as today:

- `TerminalNode`: dialled by the node (role Node).
- `TerminalUi`: dialled by the UI (role Ui).

```
TerminalFrame { oneof body {
  Attach  attach = 1;   // first frame, from the dialler only: { terminal_id }
  bytes   data   = 2;   // opaque; at most 16 KiB. Output when sent by the node, keystrokes when sent by the UI
  Resize  resize = 3;   // UI -> node only: cols, rows (same bounds as open)
  Exit    exit   = 4;   // node -> UI only, last frame: { reason, status }
  Close   close  = 5;   // UI -> node only, last frame: the UI is done
} }
Exit.reason: CLIENT_EXITED | START_FAILED | CLOSED_BY_UI | NODE_LOST | STALLED | REVOKED | SHUTDOWN
```

**Boundary validation.** Every peer-controlled field is validated where it enters, before it reaches a queue: `cols`/`rows` nonzero and at most 1000 (in `OpenTerminal` and `Resize`); `term` 1 to 32 bytes of `[A-Za-z0-9._-]`; `data` at most 16 KiB (checked on receipt, in the orchestrator and again at each end); the *direction* of every frame (a node cannot send `resize` or `close`, a UI cannot send `exit`); the first frame must be `Attach`. A violation ends the session with a protocol error. The orchestrator carries and copies the `data` bytes, but treats the payload as opaque: it never interprets ANSI sequences, terminal semantics or application content, and never logs it.

Golden-hex wire tests and the schema drift test cover the new messages like every other.

#### Correlation and routing

1. The UI sends `OpenTerminal` on its normal command channel. `route()` does what it does for every command (node known, connected, capability accepted) and additionally **mints `terminal_id`** (128 random bits) and records `{terminal_id, authenticated UI identity, node id and connection, pane_ref, expected_pid, state: Opening, expiry}` in a terminal table. The id is written into the forwarded command, so the node learns it from the orchestrator and no peer chooses it.
2. The node validates, starts the PTY (below), answers `Done`; the orchestrator turns that into `TerminalOpened { terminal_id }` for the UI (and refuses to answer success if the entry is gone). Any failure answers with the typed error and deletes the entry; no PTY exists on failure.
3. The node dials `TerminalNode` and sends `Attach { terminal_id }`; the UI, on receiving `TerminalOpened`, dials `TerminalUi` and sends `Attach { terminal_id }`. The orchestrator accepts each only if the authenticated identity equals the one recorded for that id and that side has not attached yet: **a `terminal_id` is single-use**, at most one authenticated UI attachment and one authenticated node attachment ever. After an attach failure, timeout, close, disconnect, revocation or exit the id is permanently dead (the entry is deleted and ids are never reissued; 128 random bits make a guessed or replayed id equal an unknown one). Both sides must attach within 10 s of the open request or the entry is deleted: the node kills its PTY, the UI reports "terminal did not start". An unknown, expired or already-used id is refused identically to an unauthorized one (no oracle).
4. Once both sides are attached the orchestrator splices the two streams. The entry is removed when either stream ends.

The orchestrator holds, per terminal, two bounded queues and an id. No terminal state is replicated or persisted; an orchestrator restart ends every terminal.

#### Backpressure and limits

- **No unbounded buffer anywhere.** Each direction in the orchestrator is a queue of 4 frames (at most 64 KiB); the node's PTY reader hands at most one 16 KiB chunk at a time to its sender; the UI writes to its terminal synchronously. gRPC/HTTP/2 stream windows bound what is in flight on the wire.
- **Slow UI.** The UI stops reading, so the orchestrator cannot write to it, so its queue fills, so it stops reading the node stream, so the node's HTTP/2 window closes, so the node stops reading the PTY master, so the PTY buffer fills and the tmux *client* blocks writing. tmux's own handling of a blocked client terminal (it stops writing to it and redraws when it drains) is what keeps the cost off the agent's session; this must be confirmed before the slice is accepted by a test that wedges the UI side for a minute while the pane produces output continuously, and verifies **bounded memory and queues at every stage**: the UI relay holds at most one frame, the orchestrator's two queues stay at their cap (asserted from a counter, not inferred), the node's pump holds at most one chunk, and the tmux server and tmux client resident memory stay within a fixed margin of their pre-wedge size; the pane's own program keeps running; then the UI is released and the stream drains or is torn down cleanly, leaving no PTY, child process or table entry.
- **Slow node, slow input.** Keystrokes use the same mechanism in reverse; a paste larger than the pipeline simply takes as long as the node takes to consume it. Nothing is dropped.
- **Stall rule.** If no frame moves in a direction that has data waiting for 30 s, the orchestrator ends the session (`STALLED`). HTTP/2 keepalive on the terminal connections (10 s ping, 20 s timeout) detects a dead peer or a UI machine that went to sleep.
- **Limits** (constants, configurable later): 4 terminals per node, 2 per UI identity, 32 per orchestrator. Beyond a limit, `OpenTerminal` fails at once with `Busy` and no PTY is created; the limit counts entries in any state, so a client cannot hold slots by never attaching (the 10 s deadline frees them). At most one terminal per (UI, pane): a second open for the same pane ends the first.

#### What the node launches

On `OpenTerminal` the node, in `TmuxServers`, with no job started before step 1 passes:

1. Refuse unless `terminal_v1` was accepted for this peer, a slot is free, and the published pid equals `expected_pid` (the same pre-job check as reveal).
2. Create the PTY (`openpty`, window size = `cols` x `rows`), and run, with an empty environment except `PATH`, `HOME`, `LANG`/`LC_ALL` set to a UTF-8 locale, `TERM` = the validated request value (no `TMUX`, no `TMUX_PANE`, nothing else inherited), with the PTY slave as controlling terminal in a new session (`setsid`, `TIOCSCTTY`):

```
tmux -u -L <server> if-shell -t %<pane> -F '#{==:#{pane_pid},<expected_pid>}' \
     'select-window -t %<pane> ; select-pane -t %<pane> ; attach-session -t %<pane>' \
     "run-shell 'exit 1'"
```

`<server>` is the node's own configured socket name, `%<pane>` is a pane id from its own tmux listing, and `<expected_pid>` is the validated integer: no string from a peer is placed into the command. No `-d` (other clients of the session are never detached) and no `-r` (the user must be able to type).

**Revalidation is atomic with the attach.** The node checks the pid against tmux first (the refusal that gives `PaneChanged` and creates nothing) and then tmux checks it *again inside the single command that attaches*: the condition, the selection and the attach are one tmux command queue, so there is no window between "still the same process" and "client attached". Verified against tmux 3.7b: `attach-session -t %<pane>` attaches to that pane's session with that pane selected; with the right pid the client attaches; with a wrong pid the command exits 1 and no client ever appears in `list-clients`. A tmux client that exits within its first moments without having attached is reported as `START_FAILED` (late `PaneChanged` included); the exact exit-code distinction is not relied on.
3. Answer `Done`, register the PTY under `terminal_id`, wait for the stream (10 s), then pump: PTY master to `data` frames, `data` frames to the PTY master, `resize` to `TIOCSWINSZ` (tmux sees SIGWINCH).
4. When the tmux client exits, send `Exit { CLIENT_EXITED, status }` and end the stream; reap the child.

The minimum tmux version is the one that supports `if-shell -F` with `attach-session` as shown (3.x; CI covers the ubuntu and macOS versions). The node does not become a tmux client of its own control connection; the control-mode observer keeps discounting only its own client.

Other clients of the session see the reveal (the session's current window changes for everyone attached) and, with tmux's default window size policy, a smaller remote terminal can resize the window for them. That is tmux's normal multi-client behaviour, not something Flight adds; it is documented, not worked around.

#### What the UI does

Opening is Enter's second stage. The UI suspends the dashboard (leaves the alternate screen, raw mode), connects the stream, then copies terminal input to `data`, `data` to the terminal, and size changes to `resize`; it **returns to the dashboard** when the terminal ends (not exit: the dashboard is the user's home). It ends the terminal itself on a local escape that is handled before forwarding, so a wedged link can always be left and a literal key is never impossible to send: `Ctrl-]` then `q` leaves; `Ctrl-]` then `Ctrl-]` sends one literal `Ctrl-]`; `Ctrl-]` followed by anything else is discarded together with a one-line hint (nothing is forwarded by accident). It also ends on stream loss and when the node reports `Exit`; it always prints why before redrawing. The remote tmux's own detach (prefix, `d`) also works and ends with `CLIENT_EXITED`.

#### Lifecycle

| Event | Result |
|---|---|
| Stale pane at open (pid differs, pane gone) | `PaneChanged` / `UnknownPane` from the node before anything is created. |
| Atomic re-check fails after the PTY was created | The tmux client exits; `Exit { START_FAILED }`; the UI shows "the pane changed; refresh". |
| User detaches in the remote tmux, or the session is killed, or the tmux server exits or restarts | The tmux client exits; PTY EOF; `Exit { CLIENT_EXITED, status }`. The session and agent are unaffected by a detach. |
| UI presses its detach key, quits, crashes, or loses its link | The UI stream ends (or `Close`); the orchestrator ends the node stream; the node closes the PTY master, which hangs up the tmux client (detaches cleanly), and reaps it. The tmux session and agent are untouched. |
| Node loses its control link to the orchestrator | The orchestrator ends every terminal of that node connection (`NODE_LOST` to the UI); the node, whose terminal streams also end, hangs up the PTYs. No terminal outlives its node's registration. |
| Node's terminal stream breaks alone | Same as above from the node side: PTY hung up, the UI gets `NODE_LOST`. |
| Orchestrator restarts or stops | All terminal streams error; the UI returns to the dashboard with "terminal lost"; nodes hang up their PTYs. No resume or replay in v1 (it would need a replay buffer and an ordering protocol); the user presses Enter again and gets a fresh tmux client on the same session. |
| Node forgotten or revoked | The orchestrator ends its terminals (`REVOKED`). |
| Stalled or half-dead peer | Stall rule and keepalive above (`STALLED`). |
| Limit reached | `Busy` at open; nothing created. |

Every ending path must leave no PTY, no child process and no table entry: a test per row checks the node's process table and the orchestrator's table afterwards.

### Trust

Names that a node supplies (its display name, session and window names) are data, never routing. Nothing the UI executes comes from a node: the remote path executes nothing on the UI's machine at all, it only draws bytes in the terminal the user already has. The node derives the tmux target from its own tmux listing (the pane id and pid, checked against tmux), never from a session name in a request.

Remote presentation lets a UI type into a pane on a node, which is more than the preview and reveal it could do before. It is gated the same way every other control capability is (UI role identity, enabled in the trust store, capability negotiated) and by one more switch on the node: it advertises `terminal_v1` unless started with `--no-terminal`. The default is on for an authorized node, consistent with the authority an enrolled UI already has (including `create_session`, which starts an arbitrary command on a node); `--no-terminal` removes the capability for nodes whose operator wants a read-only fleet. Fine-grained permissions are a future design if Flight becomes multi-user. The orchestrator logs one line per terminal open and close (UI identity, node, pane, reason, duration). The orchestrator handles terminal bytes in the clear (it copies them between streams, opaquely), as it already sees previews; end-to-end encryption between UI and node is out of scope.

### Failure behaviour

| Failure | Result |
|---|---|
| Node unknown, disconnected, or without `guarded_reveal_v1` / `terminal_v1` | Immediate typed error from the orchestrator; message names the node and the cause. |
| Pane replaced or gone | `PaneChanged` / `UnknownPane`; nothing is changed. |
| No answer within the orchestrator's request timeout (10 s default, reported as `NodeUnreachable` "request timed out"; there is no separate timeout code), or the node or orchestrator link drops mid-request | "no confirmation"; the reveal may or may not have happened (harmless); no attach is started. |
| Terminal cannot be opened or ends after a successful reveal | The typed reason is shown (`Busy`, `START_FAILED`, `NODE_LOST`, ...); the pane stays selected; the dashboard is still there. |

The dashboard exits only after `SwitchClient` succeeds or when it hands the terminal to a *local* attach. A remote terminal ends back in the dashboard.

## Superseded

An earlier revision of this note presented a remote pane with `ssh -t <destination> tmux attach`, with the destination read from an operator-owned `ui/ssh.toml` keyed by node id. It was implemented in slice 4 and is withdrawn: it needs ssh identities and configuration next to Flight's own, a direct network path from the UI machine to every node, and a second trust system, and it fails for nodes reachable only through their outbound connection. Reveal, `expected_pid`, `PaneChanged`, `guarded_reveal_v1`, routing and client identification are unaffected.

## Not in scope

- `SendInput` as a separate programmatic command: different security and UX questions (text injection, confirmation, secrets in logs). Its own ADR. (A human typing in a presented terminal is covered above.)
- Terminal resume or replay across a lost connection, session sharing between several UIs, end-to-end encryption between UI and node, per-pane terminal permissions.
- Key rotation, process-table and hook discovery, the third-machine UI test.
- **`KillPane` is under-guarded today** (found while writing this): it compares the node's published pid with tmux's, but not with the pid the caller saw. The UI has no kill action yet, so nothing is exposed. It must adopt the rule above (`expected_pid`, a guarded capability) before any kill action is added to a UI.

## Slices

1. This note.
2. **Done.** Protocol (`PaneState.pid`, `RevealPane.expected_pid`, `PaneChanged`, `guarded_reveal_v1`) and the node-side reveal with the pid guard, against real tmux (selects window and pane, touches no client, refuses a replaced pane and a pane id reused by a new server). The popup experiment is recorded above.
3. **Done.** Orchestrator routing: the existing control path already checks node, liveness and `guarded_reveal_v1` and forwards the command unchanged; tests pin that `expected_pid` is never touched, the node's `PaneChanged` reaches the UI, and a disconnect or timeout fails the request while a late answer from the dead connection is ignored.
4. **Local presentation done; remote presentation superseded.** UI (`flight-client/src/switch`): `plan_switch` (pure, table-driven tests), `Switcher` (reveal then present, staged errors), client identification, the Enter path and the local executors (`LocalClient`, `LocalAttach` with a `Handoff`). Verified with pty-attached clients: one client, two clients, a real `display-popup` (empty `$TMUX_PANE`, session from `$TMUX`), and a control client that must not count. The local node identity is read from `<config-dir>/node` (`--node-dir` overrides); none means every pane is remote.
   - **Kept:** `PaneView.pid`, `Backend::switch_to(&PaneView)`, `SwitchTarget`, `plan_switch` (its local branches), `UiPlacement`/`detect_placement`, `Refusal` (minus the ssh variants), `SwitchError`, `Handoff`/`HandoffSlot` (local attach only), `Switcher` (local arms, staging), `flight-tmux` `ClientInfo`/`list_clients`/`session_id_of_pane`/`switch_named_client`, `--node-dir` local identity, the live pty tests.
   - **Removed by slice 4b:** `ui/ssh.toml` and `SshDestinations`; `SwitchPlan::RemoteAttach { ssh_alias }` (becomes `RemoteTerminal { host, server, target }`); `UiContext.ssh`; `Refusal::{NoSshDestination, UnsafeSshDestination}` and the `ssh` program check; the ssh arm of `Switcher`; `SshRunner::attach_args` (its only caller); the `ssh.toml` text in `flight ui` help; `tests/remote_switch.rs` (rewritten for the terminal path). `SshRunner::check_alias` and the rest of `SshRunner` stay: the v0 direct (non-distributed) host registry still uses them and they are not part of the distributed path.
   - **No ssh fallback is retained.** An optional fallback would be dead, untested code on a path nobody can reach by default; if one is ever wanted it is its own decision with its own opt-in flag, and nothing in this design depends on it.
4b. **Done.** The ssh remote path, `ui/ssh.toml`, `SshRunner::attach_args` and their tests are removed. The remote branch of Enter is refused before anything is revealed ("not available yet") until slice 9.
5. **Folded into earlier and later slices.** The local cases (selected pane moves the one client, replaced pane, several clients, outside tmux) are the live pty tests of slice 4; node disconnect mid-request is the routing test of slice 3; the remote cases are slice 10.
6. **Done.** Protocol: `OpenTerminal`, `terminal_v1`, `TerminalOpened`, `Busy`, `TerminalFrame` and the two RPCs, with golden-hex and schema-drift tests.
7. Node: PTY, the guarded tmux command, pumps, limits, lifecycle; against real tmux, including the atomic-recheck cases (right pid, wrong pid), every lifecycle row, and the process-table check.
8. Orchestrator: terminal table, id minting, attach authentication, splice, direction/size checks, limits, stall rule, node-loss and revoke handling; core-level tests with the in-memory world, then transport tests with real streams (including the wedged-UI backpressure test).
9. UI: terminal presenter (suspend dashboard, raw relay, resize, local detach key, return to dashboard), plan `RemoteTerminal`.
10. End to end: remote selected pane through a real orchestrator and node with no ssh anywhere (the test environment has none configured), the pane-replaced race across the whole chain, node disconnect mid-session, orchestrator restart mid-session, UI wedge.
