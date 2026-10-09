<!-- SPDX-License-Identifier: MIT -->

# Plan: persistent workspaces

Each increment is independently testable and shippable; later ones need earlier ones. See ADR-008, ADR-009.

| # | Increment | Tests | State |
|---|---|---|---|
| 1 | `flight-workspaces`: schema, atomic store, snapshots, migration, root probe, planner, recovery driver, trust policy | store crash/corrupt/newer/migration; roots missing/unmounted/permission/reappear/changed/git; recovery repeat, lost reply, unreachable host, ambiguity, trust | done, PR #13 (merge ad257df) |
| 2 | Node wiring: `@flight_config` marks written by `CreateSession` / `CreateSurface`; node `Observer` over `TmuxServers`; autosave on create; load at start; `FsProbe` against the node's home | live tmux (private socket): create, kill server, recover once; repeat recover | done, PR #14 (merge 652ae3a) |
| 3 | Protocol: node publishes saved workspaces with health and root diagnostics (capability `saved_workspaces_v1`; old node publishes none) | proto compat, old/new peer matrix, orchestrator relay, real-TLS end to end, live tmux report | done, PR #15 (merge 68b1091) |
| 4a | Dashboard shows saved workspaces that are not running, with host, configured root and failure; selectable; details in the preview pane | view-model, render (every failure kind, narrow terminal, no backend words) | done (this PR) |
| 4b | Actions on a saved workspace: retry now, change root, accept a changed root, remove, restore (start); wire commands routed by saved key, capability `saved_actions_v1` | proto, orchestrator routing, node live, UI | next |
| 5 | Snapshots and profiles from the UI; export/import with the import trust prompt | | |
| 6 | Persistent surface transport experiment (ADR-009) | keystroke-loss and latency measurement | |
| 7 | Agent-session resumption for a provider that supports it | | |
| 8 | Terminal emulation / composition for side by side | | |

Separate tracks: the two flaky tests are tracked outside this work; Git worktree lifecycle is a different feature (ADR-008, "Git").
