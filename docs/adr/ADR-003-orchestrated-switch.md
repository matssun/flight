<!-- SPDX-License-Identifier: MIT -->

# ADR-003: Switching to a pane through the orchestrator

Status: Proposed (design note; nothing implemented yet)

## Problem

Enter on a pane in the orchestrated dashboard says "not available yet" (ADR-001 limit, ADR-002 "Not yet"). The protocol already has `SwitchPane` and the `switch` capability; the node refuses it. The hard part is not routing: `tmux switch-client` acts on a tmux client, and "the client" is a property of where the user's terminal is, which is not where the pane is when the pane is on another machine.

## Decision

**"Switch" is two operations with different owners.**

1. **Reveal** (server state, owned by the node): make the target the active pane of its window and its window the current window of its session. `select-window -t @W` then `select-pane -t %P` on the node's own tmux server. A node is a service with no tmux client and no way to know which client is the user's, so it never touches clients.
2. **Present** (client state, owned by the machine the UI runs on): point a tmux client at that session. Only the UI's machine can do this.

The orchestrator only routes the reveal (the existing control path: capability check, immediate typed failures for unknown, disconnected or incapable nodes). Presenting never crosses the orchestrator.

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

No "pick any attached client". The UI asks tmux which client it is: `display-message -p -t $TMUX_PANE '#{client_name}'` run against the server named by `$TMUX`, and confirms that exactly one attached client of that pane's session matches. Zero or several matches (for example two terminals attached to the same session) is `Refuse("cannot tell which tmux client shows this dashboard")`. Whether this resolves correctly for a `display-popup` is checked against real tmux in slice 2 before anything else is built on it; if popups do not resolve, popups are `Refuse` with a message, not a guess.

### Which machine is "this machine"

The node role directory under the UI's config dir (`<config-dir>/node/`, joined to the same orchestrator) gives the local node's `HostId` (its identity fingerprint, which is what `PaneRef.host` carries). `--node-dir` overrides it. No node role on this machine means there is no local plan: every pane is `AttachRemote` or `Refuse`. Matching by pane id and pid alone is not used: it would treat a coincidentally equal `%12` on another machine as local.

### Stale-pane guard

A pane id is reused across process lifetimes. Today's `KillPane` guard compares the node's *published* pid with tmux's current pid, which protects against tmux changing under the node but not against a UI that is looking at an older picture than the node.

For switch the request carries the pid the UI saw, and every step checks it:

- `PaneState` gains `pid` (additive; it changes only when the process does, so it adds no per-poll delta). The UI view model carries it.
- `SwitchPane` gains `expected_pid` (additive, validated non-zero).
- The node refuses unless published pid, tmux's current pid for that pane, and `expected_pid` all agree, with a new `PaneChanged` failure (not `UnknownPane`) so the UI can say "the pane changed, refresh" and not "no such pane".
- The local plans run the same check against tmux directly.

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
- **Found while writing this:** `KillPane` has the stale-UI hole described above. The UI has no kill action yet, so nothing is exposed today; the `pid` added here is what lets `KillPane` carry `expected_pid` before such an action exists.

## Slices

1. This note.
2. Protocol (`PaneState.pid`, `SwitchPane.expected_pid`, `PaneChanged`) and the node-side reveal with the pid guard, against real tmux. Includes the client-identification check for popups.
3. Orchestrator routing: the `switch` capability offered by nodes, typed immediate failures, request timeout.
4. UI: `plan_switch` (pure, table-driven tests), executors (`switch-client`, attach by exec), the Enter path and its messages.
5. End to end: local selected pane, remote selected pane, pane replaced before the request, no suitable client, node disconnect mid-request.
