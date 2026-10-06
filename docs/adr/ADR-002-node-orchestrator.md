<!-- SPDX-License-Identifier: MIT -->

# ADR-002: Node / orchestrator architecture

Status: Accepted with refinements (see "Sign-off refinements"). Slice 1 (`flight-proto`) implemented.

## Decision

Flight moves from SSH-based multi-host access to a node/orchestrator architecture. No further SSH features are added; `SshRunner` is kept (see "What stays").

    flight-ui -> flight-orchestrator -- Flight protocol -- flight-node -> tmux / hooks / ps / events

- **flight-node** runs on every machine. It owns local observation: tmux access, process/hook/event collection, `classify -> fuse -> resolve`, and per-pane `Tracking`. It exposes compact resolved pane state plus on-demand capture and control.
- **flight-orchestrator** is the live control plane: node registry, health, routing, current pane states, subscriptions. It is not a storage server and not an execution host.
- Nodes dial outward, one long-lived bidirectional connection each, even on the LAN. A cloud orchestrator later is a deployment change, not an architecture change.
- The orchestrator machine also runs a `flight-node` that connects like any other. Nothing special-cases "the local machine".
- LAN v0 uses an explicitly configured address.

Out of scope for v0: mDNS, leader election, Raft, relays, NAT traversal, persistence of live state, scheduling ("start X" without naming a node).

## Sign-off refinements

These supersede the earlier text where they differ.

- Transport: tonic (gRPC) + rustls/mTLS. Request correlation, deadlines, status codes and flow control come from the stack instead of being rebuilt in Flight. `flight-proto` messages are hand-written `prost` types (no `protoc` at build time) and plug into tonic's `ProstCodec`; a `.proto` file for non-Rust clients is deferred.
- Pairing keeps two separate concepts: the orchestrator identity (stable public-key fingerprint, which the node authenticates against) and an independent short-lived single-use enrollment token (which authorizes joining). A human-copyable bootstrap bundle may carry both; the protocol models them separately. SPAKE2 is not used initially.
- The UI is a client of the orchestrator over a separate interface (`UiRequest`/`UiEvent`), not a role on the node listener. Nodes publish state and execute control requests; a UI consumes state and issues operator commands.
- The trust store is operator-managed configuration (`~/.config/flight/trust.toml`: identities, aliases, fingerprints), not live state. Private keys are separate files with restrictive permissions. No SQLite.
- Hooks stay deferred and are node-local; they never reach the orchestrator.
- The wire carries semantic pane/node state only (`PaneState`: ref, agent kind, resolved state, provenance summary, times). Classifier internals (hook-file changes, screen matches, glyph anchors) never appear on the wire.
- Replication: `Snapshot(generation = N)`, then `Delta(generation = N, sequence = 1, 2, ...)`. A missing, duplicate or foreign-generation delta, or a reconnect, means "request a fresh snapshot"; there is no repair. Added invariant: a delta is never required for correctness; a complete snapshot always suffices to reconstruct the node's externally visible state.
- Compatibility rules, enforced in `flight-proto`: major mismatch rejects the connection; unknown optional fields are ignored; unknown capabilities are negotiated away; an unknown or unspecified enum value rejects that message and is never guessed.
- `GetHostStatus` is dropped; per-server availability is carried by `ServerStatus` in snapshots and deltas.

## Invariants

1. The orchestrator may disappear without affecting any running workload (tmux, agents and nodes keep running).
2. The orchestrator can reconstruct all required live state from connected nodes alone.
3. LAN discovery is unauthenticated; Flight connections are mutually authenticated and encrypted. Discovery never implies trust.
4. `NodeId` is stable cryptographic identity; hostnames are mutable display names.
5. No Flight component assumes the user's default tmux server (unchanged from ADR-001).
6. Terminal contents never flow continuously; only semantic state does.

Invariants 1-2 are the guard against the orchestrator becoming a database: anything that cannot be rebuilt from a node's resync does not belong there.

A distinction on invariant 2: the *trust store* (which node keys are authorized) is configuration, not live state. It is small, file-based, and operator-edited, and its loss means re-pairing, never lost workload state.

## Crates

| Crate | Role after this ADR |
|---|---|
| `flight-tmux`, `flight-state`, `flight-classify` | Unchanged. Used by `flight-node`. |
| `flight-proto` (new) | Wire types, versioning, capability names, encode/decode. No I/O. |
| `flight-node` (new, lib + bin) | Observation loop, Tracking, serves control requests, dials the orchestrator. |
| `flight-orchestrator` (new, lib) | Node registry, routing, subscriptions, fan-out to UIs. Transport-agnostic core over a `Session` trait. |
| `flight-control` | Becomes the client-side facade the UI uses: a `Fleet` trait (list panes + host health, preview, control) with two backends: `Orchestrated` (new) and `Ssh/Local` (today's `HostRegistry`). |
| `flight-ui` | Depends on the `Fleet` trait only. `Collector` splits: resolution moves into `flight-node`; the UI receives `PaneSummary`, not scraped screens. |
| `flight` | Binary: `flight` (UI), `flight --orchestrator`, `flight node ...`. Separate executables are a later packaging choice. |

Single-host semantics stay frozen: the node reuses the existing pipeline unchanged, so Fleet-parity goldens keep covering it.

## Tracking placement

`Tracking` (`was_busy`, `done`, `glyph_anchor`) lives in `flight-node`, resolved there and shipped as `ResolvedState`. UI or orchestrator restarts lose nothing; only a node restart forgets a pending Done, equivalent to restarting the local observation engine. Still ephemeral, still no storage. This resolves the open item in ADR-001.

## Protocol (versioned from day one)

Envelope: `Frame { protocol_version, request_id?, body }`. Encoding: Protobuf, carried as one bidirectional stream per node connection. Three traffic classes:

**Node -> orchestrator (events)**

- `NodeHello { protocol_version, node_id, display_name, capabilities, tmux_servers[] }` (first frame)
- `Heartbeat { seq }`
- `Snapshot { generation, panes[] }`: full resync; sent after hello and on request.
- `PaneChanged { pane: PaneSummary }`, `PaneAdded`, `PaneRemoved { pane_ref }`: deltas, each carrying the snapshot `generation` so a gap forces a resync.
- `ServerStatus { server, status }`: tmux server availability, typed like today's `HostError`.

**Orchestrator -> node**

- `OrchestratorHello { protocol_version, accepted_capabilities, heartbeat_interval }`
- `ResyncRequest`
- Control requests (below).

**Request / response (either direction, matched by `request_id`)**

- `GetPreview { pane_ref, lines }` -> `Preview { text, captured_at }`
- `SwitchPane`, `SendInput`, `KillPane`, `CreateSession { name, dir, command? }`, `KillServer`, `GetHostStatus`
- Response is `Ok(..)` or a typed `Error { kind, message }` using the existing `HostError` kinds plus `Unsupported`, `ProtocolMismatch`, `NotAuthorized`.

`PaneSummary { pane_ref, agent_kind, state, provenance, rule_id?, why, changed_at, session/window names, path, command }`. It is the existing resolved pane record minus screen text.

Negotiation: the orchestrator picks the highest common `protocol_version`, rejects with `ProtocolMismatch` otherwise; capabilities are named strings (`preview`, `send_input`, `hooks`, ...) and requests for an unaccepted capability fail with `Unsupported`. Unknown fields are ignored, so adding fields is non-breaking.

Preview: only the UI's selected pane is fetched, at a modest rate while selected. At 5 machines / 100 agents the orchestrator receives state for all 100 and terminal text for about one.

UI side: the UI speaks the same protocol to the orchestrator (`Subscribe`, then `Snapshot` + deltas, plus routed requests). The orchestrator is a fan-out of node events, not a re-classifier.

## Transport and security

- Transport: TCP + TLS 1.3 (rustls), bidirectional streaming. Proposal: `tonic` (gRPC over HTTP/2) with Protobuf, for an established stack and generated types. Alternative if tonic feels heavy: length-prefixed protobuf frames directly over a rustls stream. The protocol is defined independently of this choice (`flight-proto` has no I/O); the orchestrator core works over a `Session` trait.
- Identity: each node generates an Ed25519 keypair and a self-signed certificate. `NodeId = SHA-256(SPKI)` (hex, truncated for display). `display_name` is separate and mutable.
- Mutual TLS: both sides present certificates; verification is by pinned fingerprint, not by CA or hostname.
- Pairing bootstrap (no custom cryptography, only pinning plus a one-time secret):
  1. `flight orchestrator join-code` prints a short-lived, single-use code encoding the orchestrator's certificate fingerprint and a random secret (e.g. `7N4K-RP2F` plus fingerprint prefix, or a longer base32 form).
  2. `flight node join <addr> --code ...` connects, verifies the orchestrator certificate against the fingerprint from the code, and presents its own certificate plus the secret.
  3. The orchestrator checks the secret (constant time, single use, expiry), adds the node fingerprint to its trust store, and the node pins the orchestrator fingerprint.
  4. Afterwards both sides recognize each other by fingerprint only.
  A short human-typeable code with only a secret (no fingerprint) would need a PAKE such as SPAKE2; if short codes are wanted, that is the standard answer, not something to design ourselves.
- mDNS (`_flight._tcp.local`) is a later convenience that yields an endpoint to try, and never establishes trust.
- UI clients authenticate the same way (they are identities in the trust store with a `ui` role).
- Revocation in v0: remove the fingerprint from the trust store and drop the connection.

## What stays

`SshRunner`/`HostRegistry` remain as: a compatibility transport, a bootstrap/testing tool, the fallback for machines without a node, and a differential reference while the node transport is built (a node's snapshot for a pane should match what the SSH path computes). They receive no new features.

## Failure model

- Node dies: orchestrator marks it `unreachable` after missed heartbeats; panes shown stale, not deleted.
- Orchestrator dies: nodes retry with backoff; on reconnect they send `NodeHello` + `Snapshot`; the UI repopulates.
- Node restart: Tracking resets; pending Done is lost (accepted).
- Delta gap: generation mismatch triggers `ResyncRequest`.
- Orchestrator restart does not touch nodes, tmux or agents.

## Proposed slicing (each independently testable; only after sign-off)

1. `flight-proto` (done): types, versioning, validation, replication cursor, golden wire fixtures, malformed-input tests. No sockets.
2. `flight-node` core over an in-memory `Session`: wraps today's collector and resolve, emits Snapshot and deltas; tests with scripted tmux.
3. `flight-orchestrator` core over in-memory sessions: registry, resync, stale handling, routing; tests for the two invariants (restart reconstructs state; one node failing leaves others intact).
4. TLS transport and pairing, still on localhost with two processes.
5. Move `flight-ui` onto the `Fleet` trait with the `Orchestrated` backend; keep the SSH backend working.
6. Real second machine on the LAN.
7. Measure, then decide on packaging and the next UX changes.

## Open questions

1. Authentication details of the enrollment token exchange (token format, expiry default).
2. Trust-store file schema.
3. Hook ingestion path on the node (slice 2+).
