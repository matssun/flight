<!-- SPDX-License-Identifier: MIT -->

# Plan: persistent workspaces

Each increment is independently testable and shippable; later ones need earlier ones. See ADR-008, ADR-009.

| # | Increment | Tests | State |
|---|---|---|---|
| 1 | `flight-workspaces`: schema, atomic store, snapshots, migration, root probe, planner, recovery driver, trust policy | store crash/corrupt/newer/migration; roots missing/unmounted/permission/reappear/changed/git; recovery repeat, lost reply, unreachable host, ambiguity, trust | done, PR #13 (merge ad257df) |
| 2 | Node wiring: `@flight_config` marks written by `CreateSession` / `CreateSurface`; node `Observer` over `TmuxServers`; autosave on create; load at start; `FsProbe` against the node's home | live tmux (private socket): create, kill server, recover once; repeat recover | done, PR #14 (merge 652ae3a) |
| 3 | Protocol: node publishes saved workspaces with health and root diagnostics (capability `saved_workspaces_v1`; old node publishes none) | proto compat, old/new peer matrix, orchestrator relay, real-TLS end to end, live tmux report | done, PR #15 (merge 68b1091) |
| 4a | Dashboard shows saved workspaces that are not running, with host, configured root and failure; selectable; details in the preview pane | view-model, render (every failure kind, narrow terminal, no backend words) | done, PR #16 (merge eca7e43) |
| 4b-1 | Actions on the wire and node: retry, remove, restore, accept root, set root, trust; `saved_actions_v1`; client/Backend support | proto, orchestrator routing, node live (9 tests) | done, PR #17 (merge a3f26ca) |
| 4b-2 | Dashboard keys and prompts for those actions | view-model, render, keys (12 tests) | done, PR #18 (merge 597a9e2) |
| 5 | Snapshots, profiles, undo, export/import (`flight workspaces`); exclusive lock on the saved file; previous generation | library (8), node lock, CLI end to end with the real binary (8) | done (this PR) |
| 6 | Persistent surface transport experiment (ADR-009) | keystroke-loss and latency measurement | |
| 7 | Agent-session resumption for a provider that supports it | | |
| 8 | Terminal emulation / composition for side by side | | |

Separate tracks: the two flaky tests are tracked outside this work; Git worktree lifecycle is a different feature (ADR-008, "Git").
