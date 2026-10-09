<!-- SPDX-License-Identifier: MIT -->

# Status

Short and factual. Design decisions live in the ADRs; this file says where the work stands.
Last updated: 2026-10-10.

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
- Integrated (this branch): `flight-tmux` owns the control-mode connection; `flight-node` consumes a
  `PaneObserver` (`SequentialObserver` = reference and fallback, `ControlSkipObserver`); `flight node run` defaults to
  `--observer ctl-skip` at `--interval 0.5`; `--observer seq` keeps the reference path. A broken control connection
  drops every cached screen, the round is answered sequentially, and the next healthy round is a full refresh (both
  transitions appear in the node log). Our own control client is discounted from `focused`. Decisions are in ADR-002.
- Integrated-path tests: in-memory tmux with randomized histories checked against the reference; live tests on a private
  socket (agreement, focus, server restart with reused pane ids); a fault-injecting soak
  (`FLIGHT_SOAK_SECS=1800 cargo test -p flight-node --test control_skip_soak -- --ignored --nocapture`); and a smoke of
  the real binary (orchestrator + node + 20 agent panes, control client killed: unavailable then restored, 20 panes kept).
- Soak (30 min, up to 60 panes): 11119 steps, 292 control-client SIGKILLs, 19 tmux server kills, no disagreement with the
  reference (32 steps needed more than 400 ms to agree; none needed the 10 s grace). Pane count stayed low because server
  kills wipe the panes (8 alive at the end), so this soak exercises recovery far more than large-fleet steady state.
- Not done: a multi-hour soak with a steadier fleet, 500 real panes, why `ctl/events` worked after
  `ctl/all` (irrelevant to the design).

## Production observer: decided

Choose on correctness and complexity as well as CPU.

| Strategy | CPU | Latency | Stale risk | Spawns | Complexity |
|---|---|---|---|---|---|
| sequential | high | poor at scale | low | high | low |
| concurrent | high total | better | low | high | medium |
| activity skip | low | to measure | must prove | fewer | low/medium |
| control mode (all) | very low | excellent | low | none | higher |
| control + skip | lowest | excellent | low at 250 panes, 0 stale | none | higher |
| control + events | invalid | n/a | loses events outside the attached session | none | n/a |

`ctl/skip` is the node's default observer and `seq/all` stays as the fallback and reference. The capture observer
PR is next.

## Current branch: orchestrated-switch (ADR-003)

Enter on a pane in the orchestrated dashboard. Invariant (ADR-003): normal distributed operation needs no SSH
configuration and no direct UI-to-node path; everything travels over the authenticated UI/orchestrator/node connections.

- A pane of the node on this machine is shown through tmux (the one attached terminal is moved, or the dashboard is
  replaced by an attach). Client identification is conservative: exactly one attached terminal on the dashboard's
  session (control clients excluded, popups handled), otherwise the switch is refused.
- A pane of any other node: a guarded `RevealPane` (`expected_pid`, `PaneChanged`, `guarded_reveal_v1`), then a guarded
  `OpenTerminal` (`terminal_v1`, on unless the node runs with `--no-terminal`). The node runs a real tmux client in a
  PTY it owns (the pid is checked again inside the tmux command that attaches), and the terminal bytes travel
  node -> orchestrator -> UI on their own streams. The orchestrator mints single-use ids bound to the asking identity and
  the node connection, enforces limits (4 per node, 2 per UI, 32 total), and copies the bytes without interpreting them.
  `Ctrl-Space` `q` leaves, `Ctrl-Space` `Ctrl-Space` sends a literal `Ctrl-Space`; the dashboard returns with the reason.
- Output is bounded and lossy under sustained backpressure (the node keeps draining tmux, discards, then asks tmux to
  repaint); input is never dropped. Measured: without this a wedged UI grew the tmux server by about 23 MB a second.
- Verified with real tmux, real mutual TLS and the real binaries (a dashboard in a pty, a fake `ssh` that records any
  call and is never run). Known gaps: a wedged UI on an idle pane is not detected until the UI goes away; the orchestrator
  did not notice by itself a vanished node while blocked sending to a wedged UI (its stall limit is the backstop).
- `KillPane` is under-guarded and must adopt `expected_pid` before any kill action exists in a UI.

## Current branch: fleet-ui (ADR-005, ADR-006)

Usable now, from the dashboard alone, with no tmux knowledge:

- `n` opens New session (host, name, directory, Claude or Shell); the node creates it all or nothing and it appears selected.
- The dashboard is one urgency-ordered session list with a live preview, a summary header, a legend and visible controls;
  `/` searches, `?` explains the symbols, the mouse selects (second click opens) and scrolls; narrow terminals stack or use cards.
- Enter opens a session, `Ctrl-Space` then `q` returns to the dashboard, the session keeps running, Enter opens it again.

Run for real against an orchestrator and two nodes on this machine (private sockets), driving the UI in a terminal.
Not yet run on the two physical machines. Known gaps: a created shell session reads as `idle · other`; nothing on screen
inside a session says how to leave it; Question was not exercised (no fixture produces it).

## Current branch: workspace-shell (ADR-007)

The dashboard lists **workspaces**, each with an Agent surface and, on request, a companion Shell in the same host and directory
(ADR-007). Identity is a minted `WorkspaceId`/`SurfaceId` kept in tmux options, so it survives node, orchestrator and UI restarts
with no database; sessions without it appear as one-surface workspaces. `CreateSurface { workspace_id, kind }` names no host or
directory: the orchestrator routes by the workspace and the node resolves the rest. `s` offers the shell when there is none;
`Ctrl-Space a` / `s` switch inside a session (about a quarter of a second on a LAN, measured in the driven end-to-end test).

Not done: side-by-side presentation (needs terminal emulation in the client; see ADR-007), more than one shell, and any other
surface kind.

## Current branch: workspace-persistence (ADR-008, ADR-009)

`workspace-shell` (PR #11) is merged. `flight-workspaces` is the first increment: versioned `workspaces.toml` with atomic
fsynced saves, snapshots, profiles, migration steps; root verification that distinguishes missing, unmounted/unverified,
permission, not-a-directory and changed roots and never creates or repairs anything; a pure planner and an idempotent recovery
driver that reconnects, resumes or replaces under an explicit policy. Not wired into the node, protocol or dashboard yet
(see `docs/PLAN-workspace-persistence.md`).

### Increment log

| # | Commit / PR | Validation | Guarantees | Limitations |
|---|---|---|---|---|
| 1 | PR #13, merged ad257df (code 958ea5d) | fmt, clippy `-D warnings`, `cargo test --workspace`; CI 4 jobs + CodeQL green on the final head | atomic fsynced store; newer/unreadable files never overwritten; non-destructive root classification; idempotent planner/driver; trust gates | library only |
| 2 | PR #14, merged 652ae3a (code c1e7e6a) | as above, plus 10 live-tmux tests run 5 times without a failure | tmux config marks; autosave on create; startup reconcile; `--restore`; no duplicate on lost reply or concurrent passes; missing/changed directory never started in; corrupt file preserved | not on the wire or in the dashboard (increments 3-4); no agent resumption; single writer, not enforced |

| 3 | PR #15, merged 68b1091 (code f0589ef) | as above; proto schema test, 6 proto, 6 orchestrator, 4 client, 1 real-TLS and 2 live tests added | saved workspaces replicated node -> orchestrator -> UI as last-known state; capability-gated, no sequence gap for an older orchestrator; bounded and validated | UI does not display them yet (increment 4); the node reports an unusable saved file as nothing |

| 4a | PR #16, merged eca7e43 (code c92cdd0) | as above; 11 new UI tests | saved workspaces that are not running are listed in the dashboard with host, root, reason, selectable, searchable; never hidden because a host, directory or process is gone; opening one explains instead of acting | no actions yet (4b); not clickable |

| 4b-1 | PR #17, merged a3f26ca (code bc32261) | as above; 3 proto, 4 orchestrator, 9 live-tmux tests added | retry / remove / restore / accept root / set root / trust, capability-gated, routed to the named node; restore is idempotent and refuses unverified roots, untrusted imports and unprompted agents | no UI keys yet (4b-2) |

| 4b-2 | PR #18, merged 597a9e2 (code d0bc152; Ubuntu test re-run after the `control_all` flake below) | as above; 12 new UI tests | Enter starts a saved workspace, `r` looks again, `c` changes its directory, `x` forgets, `v` accepts a changed directory, `t` trusts an import; destructive or trust-granting ones ask first and default to No; a refusal keeps the question open with the reason | not clickable; no export/import UI yet (increment 5) |

| 5 | PR #19, merged c7ee7e8 (code 2141b46) | as above; 8 library, 1 node, 8 CLI tests | named snapshots, profiles, one-step undo, export/import as untrusted (new identities, host rewritten, no runtime ids or recorded identity, permission-free flag dropped, nothing starts until trusted); the node holds an exclusive lock on the saved file (a second node runs without persistence and says why; CLI edits are refused while a node runs) | CLI only (no dashboard UI for snapshots or import); solo mode does not save workspaces |

| 6a | PR #21, merged 8f4e55f | fmt, clippy, workspace tests, CI 4 jobs + CodeQL green | measurements of the surface switch (harness, delaying proxy, baseline) in ADR-009 | evidence only |

| 6b | PR #22, merged 4d63402 | as above, plus 8 live terminal tests (two terminals on two surfaces, own window, own size, no leftover view), fold unit tests, Ubuntu green | each terminal attaches to a view session of its own: keys reach only the window that terminal shows; every pane is still published once; focus kept | tmux 3.7b; views are removed by `destroy-unattached` and by the terminal's cleanup |

| 6c | PR #23 | as above, plus 25 session unit tests, 12 live session tests, 2 driven end-to-end tests, 3 orchestrator tests | the dashboard keeps its link across terminal sessions; `Ctrl-Space a/s` switches inside the session without rebuilding anything; input is an ordered, bounded log across switches; bounded re-attach; ordered teardown; input read between output frames; keys typed ahead are kept | switch 407 -> 30 ms (0 ms RTT), 1001 -> 262 ms (50 ms RTT); the terminal connection is still dialed per switch; `RevealPane` stays in the dashboard's first open |

| 6d | this PR | as above, plus connection-reuse tests (one connection for five terminals, redial after a cut, a stuck terminal does not starve another) | terminals ride a kept connection from each end; flow-control windows sized so a stalled stream cannot starve its neighbours | switch 1001 -> 198 ms (50 ms RTT), 615 -> 90 ms (20 ms), 407 -> 29 ms; 200 switches leave fds/threads flat |

| 7a | PR #25 | as above, plus tests at each layer: reference and private store, planner and recovery, the Claude adapter, and live node tests with a `claude` that behaves as the real one (resume, conversation gone, running elsewhere, wrong scope, set-root, forget, skip-permissions, newer file) | an agent is started with a session id the node chose; its reference is kept in a private file, scoped to host, root and user; a lost workspace is resumed with `--resume` in a prompting mode, after checking the conversation and that nothing runs it; a conversation that is gone is reported and nothing is started in its place; `RestoreFresh` starts a new conversation on purpose; Codex is explicitly unsupported (ADR-010) | not on the wire (7b) |

| 7b | this PR | proto validation and compatibility tests, routing test for the capability gate, client mapping, dashboard tests (wording, `f` flow, failure), node reports (available, gone, none, unsupported) | the node reports what a restart would do about the conversation; the dashboard words Enter from that report and offers `f` for a deliberate new conversation; `agent_resume_v1` | an older node shows no claim and no `f` |

| 8a | this PR | `flight-present`: 13 example tests and 4 property tests over thousands of generated layouts (tiling is exact, minimums hold, edits keep the invariants and touch only what they name, saved form round-trips and fits to the surfaces that exist) | the layout model (single, side by side, stacked, nested, tabs), geometry, focus movement, saved form, and the decision not to write a terminal emulator (vt100 as the screen model; three emulators reproduce a real tmux client's drawing exactly) | model only: no compositor or session integration yet (ADR-011 "Order") |

| 8b | this PR | 12 unit tests (attributes, colours, wide and combining characters, cursor and modes, a sequence cut between feeds, resize, 200 fuzz chunks with random resizes, drawing of two screens, tabs, a squeezed layout, a wide character at a tile edge) and 3 live tests against a real tmux client (rows equal tmux's capture, attributes, a resize) | `ScreenModel` over `vt100` and `paint` into ratatui's buffer: the surfaces' screens composed into a layout, with the focused cursor; a parser failure costs one surface its picture, not the dashboard | vt100 0.15.2 (0.16 conflicts with ratatui 0.29); not yet used by a session (8c) |

| 8c | this PR | 14 session tests (both tiles attached at their own size and drawn together; keyboard follows focus and keeps order; resize reaches each tile; close lets a surface go and a split brings it back; hidden tab not attached; per-tile exit; reconnect to the same process with input waiting; unattachable surface counts input; layout returned; nothing left to split) and key-filter, input-log and layout-edit tests | `PresentationSession`: one attachment per showing surface, ordered input log tagged by surface, per-tile resize and failure, diffed painting with the focused surface's key modes | not wired into the dashboard or saved layouts yet (8d); mouse not forwarded |

### Known flaky tests (tracked, not hidden)

- `tools/flight-load` `observe::live_tests::control_all` (scenario `server_restart_never_shows_the_old_server`: after killing and restarting a private tmux server, the control-mode observer never produced a screen within 8 s). Seen once on Ubuntu CI during the run of PR #18 (a change that does not touch that tool); 12 of 12 passes locally on macOS in isolation; the previous commit `13acdcf` already hardened this family. It is test tooling that is not shipped; the production control-mode path has its own soak. The same family failed again on a documentation-only commit of PR #20 (`observe::live_tests::control_skip`, same scenario and same assertion), so it is independent of any code change here, and it recurs on Linux CI (3 of the last ~12 runs) while passing on macOS. Not reproduced or root-caused: that needs a Linux environment, and this workspace does not use containers. Both failures were followed by a re-run of the failed job; the failures are recorded here so they are not mistaken for results of the persistence work. Recommended next step, separate from this milestone: run the scenario in a Linux container with `RUST_LOG`-style tracing of the control connection, or retire the tool test in favor of the soak that covers the shipped path.
- From earlier PRs: `a_wedged_ui_costs_bounded_memory_everywhere_and_teardown_is_clean`, `a_node_that_loses_its_link_ends_the_terminal_as_node_lost` and `n_opens_the_form_and_a_filled_form_creates_a_session_that_appears_selected` failed once each on CI under load.

### Observed failures and resolutions

- PR #20 CI: the `Analyze (python)` CodeQL job of the first head never got a runner (queued for over thirty minutes with no steps run; the other three analyses passed; the aggregate "CodeQL" check was neutral, "configuration not found"). The change was documentation only. A new head was pushed so that every analysis ran; all four, including Python, then passed and the aggregate check concluded success.

- PR #11 "CodeQL" check failed. I first assumed a config-level artifact; wrong. Cause (check-run annotation, alert #1, `rust/cleartext-logging`, high): `flight-node/tests/control_skip.rs:109`, a test fake's `panic!` printing tmux arguments that include a session id. Fixed in PR #12 by printing only the argument count; the check passed on the fix and on #13, alert #1 is `fixed`. No query was disabled.
- PR #13 could not merge when main moved (branch protection requires an up-to-date branch): merged main into the branch and waited for the checks again.
- Increment 3, found by CI on Linux (not a flake): the live test that deletes a root and makes another at the same path saw the same device and inode, so the "changed identity" check did not fire. Inode numbers are reused after deletion. `RootIdentity` now also carries the directory's creation time where the filesystem records one (older records without it still compare by device and inode); the check, the ADR and tests (skipped honestly on filesystems without creation time) were updated. Without a creation time the case remains undetectable and is documented as such.
- PR #15 CI, Ubuntu, second failure (`flight-control/tests/live.rs`, a list right after `kill_server`): tmux answered `server exited unexpectedly`, which `flight-control` and `flight-node` did not recognize as "no server" (they knew `no server running`, `error connecting to`, `failed to connect to server`). A real classification gap, not only a test race: the same moment in production (a node asked while its tmux dies) would have been reported as a failure instead of a lost server. Both classifiers now treat it as a missing server, with unit tests in each. Earlier CI failures on other PRs (`a_wedged_ui_...`, `a_node_that_loses_its_link_...`, `n_opens_the_form_...`) are different timing-dependent tests and are still tracked separately; none was touched or masked here.
- Increment 5: with the new exclusive lock, parallel live tests failed intermittently when a test dropped a node and started the next at once. A forked child that has not yet exec'd still holds the lock's descriptor, which delays release by moments (Rust opens files close-on-exec, so the window is only between fork and exec in another thread). `Store::lock_waiting` now gives an exiting process up to 2 s to let go, which also suits a supervisor restarting a node; a lock that stays held is reported as held. Repeated runs (see the increment log) are clean.
- Increment 2: changing `PANE_FORMAT` broke six fixtures that hard-code the old 19-field line (the `flight-client` differential tests failed with "not in the expected format"); the fixtures were updated, no test was loosened.
- Increment 2: the recovery driver only looked at hosts that already had a saved workspace, so a hand-made workspace on a fresh node was never recorded. Found by a live test; `recover` now takes the hosts the caller owns, with a regression test.

## Open limitations

Third-machine UI test, hooks and process-table discovery, key rotation,
branch protection on main (require the four CI checks; not yet confirmed), site-to-site VPN path MTU (1419).
