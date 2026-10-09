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
| 5 | Snapshots, profiles, undo, export/import (`flight workspaces`); exclusive lock on the saved file; previous generation | library (8), node lock, CLI end to end with the real binary (8) | done, PR #19 (merge c7ee7e8) |
| 6 | Persistent surface transport (ADR-009): measurements (6a, PR #21), per-terminal view sessions (6b, PR #22), surface session over a persistent link (6c) | latency by simulated RTT; driven key-burst end to end; live view and session tests | 6a, 6b done; 6c (#23) in review; 6d connection reuse in this PR |
| 7 | Agent-session resumption (ADR-010): 7a library, node and tests; 7b wire and dashboard | planner, adapter, live node with a `claude` that behaves as the real one | 7a #25 in review; 7b wire and dashboard in this PR |
| 8 | Composable presentation (ADR-011): 8a layout model; 8b compositor over a screen model; 8c multi-surface session and saved layouts | property tests; fidelity against tmux; driven side-by-side | 8a #27; 8b #28; 8c #29; 8d #30 |

Separate tracks: the two flaky tests are tracked outside this work; Git worktree lifecycle is a different feature (ADR-008, "Git").

## Where the persistence milestone stands

Increments 1 to 5 are merged: a Workspace survives the loss of its UI, orchestrator, node, tmux server and machine as a saved, versioned, atomically written definition that is reconciled (never blindly recreated) against what runs, with explicit unavailable states, user actions, snapshots, profiles, undo and untrusted import. Nothing in recovery creates, repairs or deletes filesystem content.

Not started, and why:

- **6, persistent surface transport.** A measurement-led experiment on a different subsystem (the terminal path), recommended by ADR-009 as the next investigation. It is not part of making workspaces durable and changes the orchestrator's terminal limits, so it gets its own plan and PRs.
- **7, agent-session resumption.** The policy and action exist (`ResumeAgent`, `resumable_providers`), and no provider is wired. The missing piece is not derivable from the repository: Flight does not know an agent's session identity (Claude's resumable session id is not published to Flight), so a provider-specific, user-visible decision is needed about how it is learned and stored without recording secrets.
- **8, terminal emulation and composition.** Deliberately after 6 (ADR-007, ADR-009).
