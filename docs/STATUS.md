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
- Harness: `tools/flight-load/scripts/observe_bench.py` (strategies: seq, conc:N, skip, ctl/all, ctl/skip).
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
- **`ctl/events` is rejected, and its measurements are invalid.** tmux sends `%output` to a control client only for
  panes of the session it is attached to (verified in isolation: 1 of 12 panes), so it cannot be a fleet-wide
  correctness signal. Its earlier clean results (and any cost comparison built on them, such as "80x cheaper")
  are an invalid measurement caused by cross-run contamination: they appeared only when `ctl/all` had run first on
  the same server (cause not understood); on a fresh server most panes ended stale. The code is removed.
- Interval sweep, 250 real panes, control mode (CPU = client + tmux server, % of a core; 0 stale panes in every run):

  | Interval | ctl/all, 5% churn | ctl/skip, 5% | ctl/all, 50% | ctl/skip, 50% | detect p50 / p95 ms |
  |---|---|---|---|---|---|
  | 250 ms | 2.1 + 2.2 | 0.8 + 1.1 | 4.7 + 8.7 | 2.9 + 7.5 | 150 / 265 |
  | 500 ms | 1.4 + 1.9 | 0.8 + 1.4 | 2.6 + 6.7 | 1.8 + 6.3 | 280 / 500 |
  | 1 s | 1.0 + 2.1 | 0.4 + 1.0 | 1.4 + 5.3 | 1.0 + 4.9 | 525 / 975 |
  | 2 s | 0.4 + 0.8 | 0.3 + 0.8 | 0.8 + 4.3 | 0.6 + 3.9 | 1030 / 1925 |

  Detection latency is half the interval at p50 and the interval at p95, regardless of strategy. At 250 panes,
  `ctl/skip` at 500 ms costs 2.6% (5% churn) to 8.1% (50% churn) of a core, less than sequential capture cost at 1 s.
  The old 1 s default only looked cheap on incomplete accounting; 500 ms is affordable with `ctl/skip`.
- Robustness (done, `tools/flight-load/src/observe`): skipping is conservative (unknown pane, new pane pid, missing
  or not-older activity: capture). Any error (connection loss, `%exit`, mismatched `%begin`/`%end`, unparseable pane
  list, failed capture, timeout) discards the cache; recovery rebuilds the connection and starts with a full refresh.
  Parser unit tests, observer unit tests and live tests on a private tmux socket cover interleaved notifications,
  server restart (pane ids reused), connection loss, high output, and pane create/remove.
- Not done: why `ctl/events` worked after `ctl/all`; integrating the observer into `flight-node`; a control-mode
  soak test (hours, thousands of server events); a decision on the node's default interval.

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

`ctl/skip` is the production candidate; `seq/all` stays as the fallback and reference path. Remaining before it
becomes the node's default: integrate into `flight-node` behind the existing observation seam, then a soak test.

## Open limitations

Third-machine UI test, orchestrated Enter/switch, hooks and process-table discovery, key rotation,
branch protection on main (require the four CI checks; not yet confirmed), site-to-site VPN path MTU (1419).
