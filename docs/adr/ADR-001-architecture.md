<!-- SPDX-License-Identifier: MIT -->

# ADR-001: Architecture

Status: Proposed

## Product boundary (added with ADR-005)

Flight is the user-facing session/workspace manager. tmux is currently an internal execution/persistence backend. Normal Flight workflows must not require users to understand or operate tmux. The decisions below describe the implementation, tmux included, as it was built.

## Decision

A Rust workspace of small crates, written fresh, using sesh, tmux-sessionizer and Fleet as design references only.

- `flight-tmux`: tmux IPC.
- `flight-state`: Fleet-style three-layer state engine (hook files, JSONL events, pane scrape) producing the 7 states PERMIT, QUESTION, DONE, BUSY, IDLE, SHELL, DOWN.
- `flight-sessions`: sesh-style sources (tmux, zoxide, config, tmuxinator), dedup, naming, connect.
- `flight-tui`: dashboard.
- `flight`: binary.

Keep Fleet's `fleet.observe/v1` JSON contract and exit codes so the two tools stay interchangeable.

## Explicit endpoints and hosts

A tmux endpoint belongs to a Flight host; neither localhost nor the default tmux socket is implicit in Flight's domain model. No component may assume the user's default tmux server.

- `flight-tmux` does one thing: perform tmux operations against one given endpoint (`TmuxEndpoint`, `-L name` or `-S path`). It does not know the fleet topology.
- Transport stays behind `TmuxRunner`: `SystemRunner` today, an `SshRunner` (`ssh host tmux -L flight ...`) next, a daemon-backed runner only if measurements show SSH is limiting, and fakes in tests.
- Identity is typed per level, never a concatenated string: `HostId`, `ServerId`, `SessionId`, `WindowId`, `PaneId`, with `PaneRef { host, server, pane }`. Display strings are derived. These live in `flight-state`; the hierarchy is host > tmux server > session > window > pane > agent.
- `flight-control` (not yet created) routes an operation on a `PaneRef` to the runner for its host.
- Phasing: local explicit socket (done), SSH, measure, then decide on a `flight-node` daemon and control-mode streaming. No network daemon until then.

## flight-control v0

`HostRegistry` routes by identity: `PaneRef -> HostId -> transport -> TmuxEndpoint -> flight-tmux`. It does not parse panes or speak tmux. Transports: `Local` (`SystemRunner`) and `Ssh { alias }` (`SshRunner`: `ssh -o BatchMode=yes -o ConnectTimeout=5 -- <alias> tmux -L <name> ...`, every tmux argument shell-quoted because ssh re-parses the command). SSH configuration is not Flight's: keys, hostnames, ProxyJump, ControlMaster and host verification stay in `~/.ssh/config`.

Failures are typed (`HostError`): `UnknownHost`, `UnknownServer`, `HostUnreachable`, `AuthenticationFailed`, `TmuxUnavailable`, `TmuxServerUnavailable`, `RemoteCommandFailed`, `InvalidConfig`. `HostStatus { reachable, tmux_available, endpoint_available, tmux_version, problem }` lets a UI render each host (online, unreachable, online with no Flight server) before any key is pressed. `list_all_panes` returns one outcome per endpoint so a down host never hides the others. No scheduling, no persistence, no daemon.

## flight-ui v0

    flight-control + flight-classify -> UiSnapshot -> ViewModel (pure) -> ratatui render (pure) -> terminal loop

The terminal loop owns I/O, the view model owns presentation state, rendering owns nothing. A worker thread runs collection so a slow or dead host never freezes the UI. Selection is a `PaneRef`, never a row index, so it follows the same agent as states reorder (and follows it out of the attention section if it stops needing attention); a vanished pane falls back to its neighbour. The attention list is a projection of the same pane records the tree shows. Host failure is part of the view (`online`, `unreachable`, `no Flight tmux server`, ...), and one down host never fails a refresh. Keys: arrows/jk, Enter (switch, then exit), Tab, r, q.

v0 limits, deliberately: agents are recognized from `pane_current_command` only (a version-named binary counts as Claude); there are no hooks or process-table discovery yet, so every agent is on the hook-less path (scrape, title, glyph, Done machine). Switching works for local panes only; a remote pane needs an ssh attach. The preview is plain text, not ANSI. Per-pane capture is one tmux call each, so remote refresh cost scales with agent count and must be measured before choosing control mode or a `flight-node`.

## Single-host semantics (frozen at parity with Fleet)

`flight-classify` is the whole observation-to-state path for one host:

    Observation -> classify (rules) -> fuse (hook/event/scrape/title) -> resolve (previous + evidence + time)

`resolve` is a pure function of `ResolveInput { previous, evidence, glyph_seen, idle_secs }`; the caller owns the per-pane `Tracking`. Hook-less panes need it for the glyph debounce and for synthesizing Done when a turn finishes while the user is elsewhere. Parity with Fleet is checked by differential goldens generated from Fleet's own code (`tools/gen_*_golden.mjs`).

Consequence for distribution: `Tracking` is in-memory and a cold start forgets a pending Done. Decision (ADR-002): it lives in `flight-node`, beside observation, so UI and orchestrator restarts lose nothing.

## Why not fork tms

tms is one crate with files well over the 200-line threshold, and is repo-centric. Fresh crates fit the workspace rules and keep copied code, and so license obligations, minimal.

## Licensing

Flight is MIT, matching all three references; see `THIRD_PARTY.md`.

## Open

Bazel build; ratcheted gates; TUI library (ratatui vs raw ANSI).
