<!-- SPDX-License-Identifier: MIT -->

# ADR-005: Create a session from the dashboard

Status: Implemented. Scope: one vertical slice. Not in scope: project discovery, git clone, templates, environment editing, history, shell selection, Claude resume, rename, arbitrary commands, presets, start on boot.

## Decision

`n` in the dashboard opens a "New session" form: host, name, directory, program (`Claude` or `Shell`). Create sends one typed request, `UI -> orchestrator -> node -> local tmux`. The node does everything; the orchestrator only routes it by `host` to a connected node that offers `create_session_v1`, exactly as it routes every other command. No SSH. No command line travels: the program is the enum `ProgramCode { Claude, Shell }`.

## Protocol

- `Command.kind` tag 8 `CreateSession { host, server, name, dir, program }`. Tag 5 (an earlier `CreateSession` that carried a free-form `command` string and was never reachable from a UI) is reserved. A peer that still sends tag 5 decodes to a command with no kind and is refused; it is never reinterpreted.
- Capability `create_session_v1` replaces `create_session`. This is needed even though nothing released uses it: nodes already running advertise the old name, and an orchestrator sending the new message to one would see `program` ignored and a Claude request quietly become a plain shell. The name carries the semantics (same reasoning as `guarded_reveal_v1`); the old name is no longer in `KNOWN`, so it is negotiated away.
- `ErrorKindCode` gains `AlreadyExists = 12`, `InvalidDirectory = 13`, `ProgramUnavailable = 14`. They only occur in answer to a `CreateSession`, which only a peer that negotiated `create_session_v1` can send, so an older peer never meets an enum value it rejects. Success is the existing `Done`.
- Bounds, checked at the UI edge, the orchestrator, the node's frame validation and again on the node before tmux: name 1 to 64 bytes of `[A-Za-z0-9_-]` not starting with `-` (tmux rewrites `.` and `:` and treats a leading `-` as an option, so what is typed is what the session is called); directory at most 4096 bytes, absolute or `~`-relative, no control characters; unknown or unspecified program refused. The rules live once, in `flight-state` (`valid_session_name`, `valid_dir`).

## Node behavior

`flight-node::session_create` validates, resolves and launches; `flight-tmux::Tmux::create_session` is the one tmux primitive.

1. Name and directory re-validated. `~` is expanded against the node's `HOME`. The directory must exist and be a directory *on the node* (`InvalidDirectory` otherwise); nothing is ever created.
2. Program. `Shell` starts nothing special: no command is passed, so tmux starts its `default-shell` as a login shell, the same as for a session made by hand. Flight does not choose or hardcode a shell. `Claude` is looked up as `claude` on the node's `PATH` (an executable regular file, absolute `PATH` entries only) and run directly by absolute path as tmux's argv, never through a shell string; a missing one is `ProgramUnavailable` and nothing is created. tmux gives a new session the `PATH` of its client, which is the node process, so the node's own service environment (not an interactive shell's) is what Claude starts with.
3. `has-session -t =name` (exact match): an existing session is `AlreadyExists` and is not touched.
4. `new-session -d -P -F '#{session_id}' -s name -c dir [argv]`. Everything after addresses the session by the returned `$id`, never by name.
5. `set-option @flight_session 1` on the new session, then a ~150 ms liveness check (`has-session -t $id`, four looks, 50 ms apart).

## Failure semantics

All or nothing, and only for the session this call created. If the mark fails or the program ends during the first moments (not executable after all, crashes on start), the session is killed by its `$id` and the answer is `ProgramUnavailable`; a session of the same name that someone else made in the meantime is never targeted. A race for the same name is tmux's "duplicate session", also `AlreadyExists`. A program that fails later than the check is an ordinary dead agent pane, not a failed create. If the orchestrator does not reach the node: `NodeUnreachable` immediately, nothing queued.

## Visibility: the `@flight_session` mark

The node publishes only panes running a known agent. A `Shell` session would never appear. Rather than publish every pane on the server, sessions Flight creates carry the tmux session option `@flight_session`; the node publishes the panes of marked sessions (as agent kind `Other`) even when they run no known agent. The mark lives in tmux, so it survives a node restart. `PANE_FORMAT` gains one field for it (`#{?@flight_session,1,0}`, before the title, which stays last). A session someone made by hand is unchanged.

## UI

The form is `ViewModel` state: pure, no I/O. `Action::NewSession` opens it (host defaults to the host of the selected pane); `Action::Form(FormInput)` drives it; a valid Create yields `Effect::Create`; the worker thread calls `Backend::create_session`; the answer returns as `Msg::Created`. Validation problems and the node's typed refusals appear inside the form, on the field to fix, and never close it. Esc or Cancel creates nothing. Success closes the form, shows "Created session X on H." and selects the new pane as soon as a snapshot contains it (the user moving the cursor first cancels that). Only hosts the dashboard has a live link to are offered. The dashboard does not enter the new session by itself.

## Consequences and awkward edges

- A node without `create_session_v1` is "too old to create sessions"; there is no fallback.
- A created Claude session starts a fresh `claude`; there is no resume.
- The liveness check cannot tell a crash at launch from a program that quits on purpose within 150 ms.
- Authentication of the `claude` started this way is whatever the node's service environment provides.
