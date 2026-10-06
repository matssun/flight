<!-- SPDX-License-Identifier: MIT -->

# ADR-001: Architecture

Status: Proposed

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

## Single-host semantics (frozen at parity with Fleet)

`flight-classify` is the whole observation-to-state path for one host:

    Observation -> classify (rules) -> fuse (hook/event/scrape/title) -> resolve (previous + evidence + time)

`resolve` is a pure function of `ResolveInput { previous, evidence, glyph_seen, idle_secs }`; the caller owns the per-pane `Tracking`. Hook-less panes need it for the glyph debounce and for synthesizing Done when a turn finishes while the user is elsewhere. Parity with Fleet is checked by differential goldens generated from Fleet's own code (`tools/gen_*_golden.mjs`).

Consequence for distribution: `Tracking` is state that must live somewhere. As in Fleet it is in-memory and a cold start forgets a pending Done. `flight-control` must decide whether Tracking lives with the central dashboard (loses Done on restart, simplest) or with a per-host node (survives dashboard restarts, needs a protocol). That decision is deferred; nothing here assumes either.

## Why not fork tms

tms is one crate with files well over the 200-line threshold, and is repo-centric. Fresh crates fit the workspace rules and keep copied code, and so license obligations, minimal.

## Licensing

Flight is MIT, matching all three references; see `THIRD_PARTY.md`.

## Open

Bazel build; ratcheted gates; TUI library (ratatui vs raw ANSI).
