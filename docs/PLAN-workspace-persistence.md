<!-- SPDX-License-Identifier: MIT -->

# Plan: persistent workspaces

Each increment is independently testable and shippable; later ones need earlier ones. See ADR-008, ADR-009.

| # | Increment | Tests | State |
|---|---|---|---|
| 1 | `flight-workspaces`: schema, atomic store, snapshots, migration, root probe, planner, recovery driver, trust policy | store crash/corrupt/newer/migration; roots missing/unmounted/permission/reappear/changed/git; recovery repeat, lost reply, unreachable host, ambiguity, trust | done, PR #13 (merge ad257df) |
| 2 | Node wiring: `@flight_config` marks written by `CreateSession` / `CreateSurface`; node `Observer` over `TmuxServers`; autosave on create; load at start; `FsProbe` against the node's home | live tmux (private socket): create, kill server, recover once; repeat recover | done (this PR) |
| 3 | Protocol: node publishes saved workspaces with health and root diagnostics (capability `saved_workspaces_v1`; old node publishes none) | proto compat, old/new peer matrix | |
| 4 | Dashboard: unavailable workspaces with host, root, reason; actions retry, inspect, change root, remove, restore | view-model and render tests; end-to-end | |
| 5 | Snapshots and profiles from the UI; export/import with the import trust prompt | | |
| 6 | Persistent surface transport experiment (ADR-009) | keystroke-loss and latency measurement | |
| 7 | Agent-session resumption for a provider that supports it | | |
| 8 | Terminal emulation / composition for side by side | | |

Separate tracks: the two flaky tests are tracked outside this work; Git worktree lifecycle is a different feature (ADR-008, "Git").
