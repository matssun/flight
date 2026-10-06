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

## Why not fork tms

tms is one crate with files well over the 200-line threshold, and is repo-centric. Fresh crates fit the workspace rules and keep copied code, and so license obligations, minimal.

## Licensing

Flight is MIT, matching all three references; see `THIRD_PARTY.md`.

## Open

Bazel build; ratcheted gates; TUI library (ratatui vs raw ANSI).
