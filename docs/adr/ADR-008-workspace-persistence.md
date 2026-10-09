<!-- SPDX-License-Identifier: MIT -->

# ADR-008: Durable workspaces, recovery as reconciliation

Status: first increment implemented (`flight-workspaces`: schema, store, root verification, planner, recovery driver, with fakes). Not yet wired into the node, the protocol or the dashboard (see the plan, `docs/PLAN-workspace-persistence.md`).

## Problem

ADR-007 derives workspaces from what the nodes publish and stores nothing. That survives a UI, orchestrator or node restart, because tmux holds the ids, but not the loss of the backend itself (tmux server killed, machine rebooted) or a root that disappears: the carefully built set of workspaces is gone, or silently shrinks. We need a durable record of intent without creating a second source of truth about what is running.

## Decision

Three things are kept apart, and each has one owner.

| | Is | Owner | Lives in |
|---|---|---|---|
| **Declared** | what the user wants to exist: workspaces, roots, surfaces | the node that owns the roots | `workspaces.toml` in the node's config dir |
| **Observed** | what is running now, and the runtime ids | the nodes, as today (ADR-007) | tmux options, published as `PaneState` |
| **Presented** | how surfaces are laid out on a screen | the UI | not persisted yet (ADR-009) |

ADR-007's "a workspace is derived, never stored" still holds for *live state*. What is added is a **desired** set that is never mistaken for live state: the dashboard shows saved-but-not-running workspaces as such, and a running workspace is "the one the saved definition names" only through the matching rules below. Neither side writes the other's facts: recovery changes the saved file only to *bind* a definition to the runtime id that realizes it, or to *record* a running workspace nobody saved.

### Why the node owns the file

Only the node can verify a root (it is the one that can `stat` it), only it can start a process there, and it must work with no orchestrator or UI present. The orchestrator stays free of durable state, as ADR-002 requires. A remote workspace is therefore saved on its own host. Moving a definition between hosts is an explicit export/import, never a sync.

### Stable identity

`ConfigKey` (`c-` and 64 random bits, minted once) names a saved workspace and each saved surface. It is not a `WorkspaceId`, `SurfaceId`, tmux id, pid or connection. Definitions carry `last_workspace_id` / `last_surface_id` as **hints**: they speed matching and are never authority.

Matching, strongest first, each tier considering only what a stronger one left unclaimed:

1. the `ConfigKey` Flight wrote into the backend when it started the workspace (tmux option `@flight_config`, `@flight_config_surface`; added with the node wiring);
2. the runtime id last seen, on an unmarked workspace;
3. an unmarked workspace in the same root.

More than one candidate, or two definitions wanting the same workspace, is `Ambiguous`: reported, never guessed. Legacy (unmarked) workspaces keep working: tier 3 adopts them, and they are recorded like any other.

### File format and evolution

TOML (`toml` is already a dependency; the file is meant to be read and diffed), `schema_version` at the top, `[[profile]]` with `[[profile.workspace]]`, named profiles with one active. Rules:

- Migration is a list of `table -> table` steps; the original is kept as `workspaces.v<N>.bak` before the upgraded file is written once.
- A file from a **newer** schema is refused and never overwritten (a downgrade must not silently drop fields it cannot see). An unreadable file is reported and never overwritten or moved implicitly; `quarantine` moves it aside intact, on the user's say-so.
- Unknown fields within a known version are ignored; a new field is additive or bumps the version.

### Crash-safe saving

Write a sibling temporary, `fsync` it, `rename` over the target, `fsync` the directory. A crash leaves the old file or the new one; leftover temporaries are never read and swept on start. Autosave is a save after every configuration change (create, remove, change root, bind, record). Removal is the same atomic write; the removed workspace's runtime id is added to the profile's `dismissed` list so that recording what is live does not put it straight back. Snapshots are named, immutable copies (`snapshots/<label>.toml`); restoring one keeps the current state under a backup label first. Single writer per file (the node); a second writer is not supported and not detected in this increment.

### Recovery is reconciliation under a policy

`plan(saved, observed, probe, policy)` is pure apart from the read-only probe and produces, per saved workspace, a **health** (`Running`, `Partial`, `Stopped`, `Blocked(why)`) and a list of **actions**. `recover` observes, plans, re-plans each workspace from a fresh observation immediately before acting, then executes. This is what makes it repeatable:

- running it again after success does nothing (the mark or binding is found);
- a start whose reply is lost (`ExecError::Unknown`) is not retried in that pass; the executor wrote the `ConfigKey` into the backend in the same step as creating it, so the next pass finds it, or finds nothing and starts it once;
- a workspace that appeared between planning and acting is bound, not started again.

The three recoveries are different actions and never conflated:

| | Action | Needs |
|---|---|---|
| reconnect to the process that still runs | none (health `Running`); `Bind` if the runtime id changed | nothing |
| resume an agent's earlier session | `ResumeAgent` | the provider supports it and is listed in the policy; **no provider is today**, so this is never planned |
| a replacement process | `StartWorkspace` / `StartSurface` | `RecoveryPolicy.start_missing` |

Starting anything is off by default. Further trust gates: an **imported** definition starts nothing unless `start_imported`; an agent saved as skip-permissions starts only with `start_skip_permissions`. Definitions hold no command line, environment or credential, so a shared profile cannot carry a secret or a program to run. Importing (`into_imported`) strips runtime ids and recorded root identity. Importing and starting are separate decisions.

### Roots: non-destructive by construction

The `Action` enum has no filesystem member, and `RootProbe` only reads. Flight never creates a missing directory, clones, creates or repairs a worktree, edits Git metadata, or deletes a saved definition during recovery. A saved workspace stays listed in every state. What the probe distinguishes:

| `RootState` | Meaning | User's options |
|---|---|---|
| `Present` | there; identity and `.git` shape recorded | |
| `Missing` | absent **and** the parent exists, is readable and is not an empty mount point | retry, change root, remove |
| `Unverified` | an ancestor is missing, or the parent is an empty directory on a mount point (what an unmounted volume looks like), or an unexpected error | retry, inspect, change root, remove |
| `PermissionDenied` | | fix permissions, retry |
| `NotADirectory` | | change root, remove |
| `HostUnreachable` | the owning node cannot be asked | retry |

`Present` is compared with the saved `RootIdentity` (device, inode) and the saved Git layout: a different directory at the same path, or a clone that became a pointer file, is `Changed`, which blocks starting until the user re-records it ("this is the workspace"). Device numbers can legitimately change across reboots on some systems, so a mismatch is never a rejection, only a question. `FirstSighting` (nothing recorded yet) is usable and is recorded.

Retry, inspect, change root and remove are user actions on the saved definition (`Profile::set_root`, `Profile::remove`); removal forgets the reference and nothing else, and saved definitions are never expired or garbage-collected.

### Git

A root is a directory. Plain directories and independent clones are both ordinary. The probe records the `.git` entry's *shape* (`Absent`, `Directory`, `File { gitdir }`, `Unreadable`) by looking at the filesystem; it never runs `git`, never requires a repository, and never assumes `.git` is a directory. A `.git` file (linked worktree, submodule) is recorded with its `gitdir:` text unfollowed; an unparseable pointer is `File { gitdir: None }`, reported and not repaired.

Boundary with future worktree management: **Flight's recovery responsibility ends at "is there a usable directory here, and is it the one we saved".** Creating, relocating, repairing or pruning worktrees is a lifecycle feature that would run *before* a workspace exists (it produces a root) and would be explicit user actions, not recovery. The model needs no change for it: a worktree is a root with a `File` marker; `RootSpec` can later gain `repo_hint` / `branch` fields additively (schema bump with a migration step). Open items for that work, deliberately not decided here: how to discover sibling worktrees (`git worktree list` is a process spawn on the node and must be opt-in), identity of a relocated worktree (inode survives a `mv` on the same filesystem; a copy does not), and what to show when `gitdir` points at a repository that moved.

## Deferred, with reasons

- Wire protocol: publishing saved-but-not-running workspaces to the dashboard, and the user actions, are the next increment (`SavedWorkspace` messages with capability negotiation, so an older node simply publishes none).
- `@flight_config` tmux marks and the node-side `Observer`/`Executor`: needed before anything starts from a definition; the crate defines the contract in the `Executor` docs.
- Agent-session resumption: needs provider support (Claude's session continuation); the policy and the action exist so adding it is additive.
- Multi-writer detection (lock file): single node process per config dir today.
- Multi-host profiles and sync: no requirement; export/import covers moving a definition.
