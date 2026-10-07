<!-- SPDX-License-Identifier: MIT -->

# ADR-003: Switching to a pane through the orchestrator

Status: Accepted. Slice 2 (protocol and node-side reveal) implemented; slices 3 to 5 not yet.

## Problem

Enter on a pane in the orchestrated dashboard says "not available yet" (ADR-001 limit, ADR-002 "Not yet"). The protocol already has `SwitchPane` and the `switch` capability; the node refuses it. The hard part is not routing: `tmux switch-client` acts on a tmux client, and "the client" is a property of where the user's terminal is, which is not where the pane is when the pane is on another machine.

## Decision

**"Switch" is two operations with different owners.**

1. **Reveal** (server state, owned by the node): make the target the active pane of its window and its window the current window of its session. `select-window -t @W` then `select-pane -t %P` on the node's own tmux server. A node is a service with no tmux client and no way to know which client is the user's, so it never touches clients.
2. **Present** (client state, owned by the machine the UI runs on): point a tmux client at that session. Only the UI's machine can do this.

The wire command is `RevealPane`, named for what it does (an earlier draft called it `SwitchPane`, which would have suggested a client switch). The orchestrator only routes the reveal (the existing control path: capability check, immediate typed failures for unknown, disconnected or incapable nodes). The dashboard's Enter composes reveal and present as two explicit stages, so a failure says which one failed ("revealed; cannot attach: no ssh destination for this node"). Presenting never crosses the orchestrator.

### The plan: a pure function of three facts

`plan_switch(pane, ui_context) -> SwitchPlan`, unit-testable with no tmux and no network. `ui_context` is: the node identity of this machine (below), whether the UI runs inside a tmux server and which, and the configured ssh destinations.

| Case | Plan | What happens |
|---|---|---|
| Pane is on this machine and the UI runs inside the pane's tmux server | `SwitchClient` | Guard locally, then `switch-client -c <ui client> -t <pane>`, then the UI exits (as the direct dashboard does). No orchestrator round trip. |
| Pane is on this machine and the UI is not inside any tmux | `AttachLocal` | Guard and reveal locally, then replace the UI with `tmux -L <server> attach-session -t =<session>`. |
| Pane is on another machine | `AttachRemote` | `SwitchPane` through the orchestrator (reveal, guarded by the node). On success replace the UI with `ssh -t <destination> tmux -L <server> attach-session -t =<session>`. |
| Anything else | `Refuse(reason)` | Nothing is changed. The message says exactly what to do. |

`Refuse` covers, each with its own message: the UI runs inside a *different* tmux server than the pane's (nesting is the user's explicit choice, never ours); the UI's client cannot be identified; no ssh destination is configured for that node; `ssh` or `tmux` is missing; the node does not offer `switch` (an older node); the node is disconnected.

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

### Trust

Names that a node supplies (its display name, session and window names) are data, never routing. In particular the ssh destination is **never derived from a node's display name**: a node could call itself `prod-db` and send the user's ssh there. Destinations come from an explicit local mapping (`<config-dir>/ui/ssh.toml`, keyed by node `HostId` with the display name shown for convenience) and are used exactly as written; ssh keys, jump hosts and host verification stay in `~/.ssh/config` (ADR-001). The remote command has a fixed shape, and the server, session and pane values in it are shell-quoted (ssh re-parses) and the pane id is validated as `%` plus digits. The UI never executes anything a node sent.

### Failure behaviour

| Failure | Result |
|---|---|
| Node unknown, disconnected or without `switch` | Immediate typed error from the orchestrator; message names the node and the cause. |
| Pane replaced or gone | `PaneChanged` / `UnknownPane`; nothing is changed. |
| No answer in 5 s, or the orchestrator link drops mid-request | "no confirmation"; the reveal may or may not have happened (harmless); no attach is started. |
| Attach fails after a successful reveal | The attach program's exit is reported; the UI stays open. |

The dashboard exits only after `SwitchClient` succeeds or when it hands the terminal to an attach.

## Not in scope

- `SendInput`: different security and UX questions (text injection, confirmation, secrets in logs). Its own ADR.
- Key rotation, process-table and hook discovery, the third-machine UI test.
- **`KillPane` is under-guarded today** (found while writing this): it compares the node's published pid with tmux's, but not with the pid the caller saw. The UI has no kill action yet, so nothing is exposed. It must adopt the rule above (`expected_pid`, a guarded capability) before any kill action is added to a UI.

## Slices

1. This note.
2. **Done.** Protocol (`PaneState.pid`, `RevealPane.expected_pid`, `PaneChanged`, `guarded_reveal_v1`) and the node-side reveal with the pid guard, against real tmux (selects window and pane, touches no client, refuses a replaced pane and a pane id reused by a new server). The popup experiment is recorded above.
3. Orchestrator routing: the `switch` capability offered by nodes, typed immediate failures, request timeout.
4. UI: `plan_switch` (pure, table-driven tests), executors (`switch-client`, attach by exec), the Enter path and its messages.
5. End to end: local selected pane, remote selected pane, pane replaced before the request, no suitable client, node disconnect mid-request.
