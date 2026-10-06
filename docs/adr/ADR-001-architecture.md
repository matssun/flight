<!-- SPDX-License-Identifier: Apache-2.0 -->

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

## Why not fork tms

tms is one crate with files well over the 200-line threshold, and is repo-centric. Fresh crates fit the workspace rules and keep copied code, and so license obligations, minimal.

## Licensing

Flight is Apache-2.0. All three references are MIT; see `THIRD_PARTY.md`.

## Open

Bazel build; ratcheted gates; TUI library (ratatui vs raw ANSI).
