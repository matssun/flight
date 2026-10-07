<!-- SPDX-License-Identifier: MIT -->

# Status

Short and factual. Design decisions live in the ADRs; this file says where the work stands.
Last updated: 2026-10-07.

## Merged baseline (main)

- Pipeline: flight-tmux, flight-state, flight-classify + fusion + temporal resolver, flight-control, ratatui flight-ui.
- Distributed v1 (ADR-002): node, orchestrator and UI over mTLS with stable node identity, enrollment and trust,
  snapshot/delta replication, resync, bounded queues, forget separate from revoke, distributed preview.
- Validated on two physical machines on a LAN. `flight node run` is a service process: run it under
  launchd/systemd, not from a tmux or ssh session. Under launchd, 8/8 Wi-Fi toggles recovered without a restart.
  `--exit-after-link-down` is an optional safety net (exit 75 plus launchd restart works), not part of the standard plist.
- CI (PR #2, merged at 12db071): fmt, clippy, test on ubuntu and macOS.

## Current branch: efficient-pane-observation

Question: how should one node observe 10-500 tmux panes efficiently and correctly?

- The earlier "a few percent CPU at 100 panes, 1 s" omitted the short-lived `tmux capture-pane` processes.
  Counted properly, sequential capture is about 29% of a core plus a few percent in tmux. The 2 s to 1 s polling
  default was chosen on the incomplete numbers and must be revisited with the observer decision.
- Harness: `tools/flight-load/scripts/observe_bench.py` (strategies: seq, conc:N, skip, ctl/all, ctl/events).
- Measured, 250 real panes, 1 s interval, 20 s runs (CPU = client + tmux server, % of a core):

  | Strategy | 5% churn: round ms, CPU, stale | 50% churn: round ms, CPU, stale |
  |---|---|---|
  | seq/all | 680, 57 + 13, 0 | 690, 58 + 15, 0 |
  | conc:8/all | 137, 81 + 11, 0 | 137, 80 + 14, 0 |
  | seq/skip | 80, 8 + 3, 0 | 452, 36 + 11, 0 |
  | ctl/all | 21, 1.1 + 2.9, 0 | 31, 1.3 + 6.9, 0 |
  | ctl/skip | 2, 0.3 + 0.6, 0 | 15, 0.7 + 4.8, 0 |

  Detection latency p50 is about 500 ms for every cheap strategy: half the poll interval, not a strategy effect.
  Concurrent capture shortens the round but costs more CPU than sequential (spawn cost is not removed).
- **`ctl/events` is invalid and must not be used.** tmux sends `%output` to a control client only for panes in the
  session it is attached to (verified in isolation: 1 of 12 panes). Its earlier clean results appeared only when
  `ctl/all` had run first on the same server (cause not understood); on a fresh server it leaves most panes stale.
  Leading candidate is now control mode with `window_activity` skipping (`ctl/skip`).
- Not done: polling-interval sweep, control-mode robustness tests (list below), why `ctl/events` worked after `ctl/all`.

Real panes are capped by the macOS pty limit (`kern.tty.ptmx_max` = 511). Runs near it (480/1000 panes) were
discarded. Synthetic node state scales past 1000; real tmux panes do not, and that is a host limit, not a Flight one.

## Decision still open: the production observer

Choose on correctness and complexity as well as CPU.

| Strategy | CPU | Latency | Stale risk | Spawns | Complexity |
|---|---|---|---|---|---|
| sequential | high | poor at scale | low | high | low |
| concurrent | high total | better | low | high | medium |
| activity skip | low | to measure | must prove | fewer | low/medium |
| control mode (all) | very low | excellent | low | none | higher |
| control + skip | lowest | excellent | low at 250 panes, 0 stale | none | higher |
| control + events | invalid | n/a | loses events outside the attached session | none | n/a |

Control mode replaces the simple path only after tests cover: tmux server restart, control connection loss,
malformed or partial messages, `%output` interleaved with replies (one such bug already found and fixed),
pane creation and removal, high output volume, reconnect and resnapshot.

## Open limitations

Third-machine UI test, orchestrated Enter/switch, hooks and process-table discovery, key rotation,
branch protection on main (require the four CI checks; not yet confirmed), site-to-site VPN path MTU (1419).
