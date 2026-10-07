<!-- SPDX-License-Identifier: MIT -->

# ADR-002: Node / orchestrator architecture

Status: Accepted with refinements (see "Sign-off refinements"). Slices 1 (`flight-proto`), 2 (`flight-node` core) 3 (`flight-orchestrator` core) and 4 (`flight-trust`, `flight-transport`) implemented.

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
- `proto/flight.proto` is the normative wire schema. The `prost` types stay hand-written (no `protoc` at build time), and `flight-proto/tests/schema.rs` fails if message names, field names and numbers, types, oneofs or enum values drift between the two.
- The replication epoch is an incarnation, not a counter: 16 random bytes per producer process, never persisted. A restarted node gets a new one, so old messages can never be mistaken for a continuation. `Snapshot` and `Delta` carry `incarnation`; `sequence` restarts at 1 after every snapshot.
- Fields that change on every poll are not replicated: `observed_at` and the terminal title (tags 7 and 13 are retired). Liveness comes from heartbeats and snapshots; otherwise every poll would be a delta storm. `changed_at` is when the state value last changed.
- `NodeCore` owns authoritative current state, not history. Deltas are state replacement (`PaneUpsert`, `PaneRemoved`, `ServerStatus`), derived by diffing state before and after a round. `Tracking` is node-local and never replicated. A pane id is keyed with its pid, so a reused tmux `%id` under a new process starts with fresh Tracking.
- A tmux server that is gone (`NoServer`, tmux missing) drops its panes and their Tracking; a transient failure keeps both (stale) and only changes the server status.
- Identity: `HostId` is the stable `NodeId` (key fingerprint once keys exist); every `PaneRef` is rooted in it and routing uses only it. The transport authenticates the peer and hands the orchestrator its id; a node's hello must agree with it, and every pane in its snapshots and deltas must carry that host. Display names are mutable presentation: two nodes may share one, and a rename changes nothing about identity or routing.
- Liveness is separate from state. `NodeStatusCode` is `Online | Stale | Disconnected`; missed heartbeats and dropped connections change only that, never a pane's state (`Down` keeps its classifier meaning). A dropped connection never removes a node: its last-known image stays visible, so selection is stable across a Wi-Fi glitch. Pruning is a separate explicit policy; `NodeRemoved` exists on the wire for it and is not yet emitted.
- The orchestrator has its own incarnation (one process lifetime), carried by `FleetSnapshot`/`FleetDelta`. After an orchestrator restart a UI can never continue an old delta stream; it takes a full `FleetSnapshot`. Each UI subscriber has its own delta sequence, restarted by its snapshot.
- Node images are transactional: a delta is validated (message, incarnation, sequence, host identity) before it touches the image. A gap keeps the last consistent image, asks the node for one snapshot, and ignores deltas until it arrives. Invariant: every node image a UI sees is a prefix of an accepted node replication stream. A reconnect, even with the same incarnation, resets the cursor: a snapshot comes first.
- Fleet deltas are state-oriented (`NodeUpsert`, `NodeStatus`, `PaneUpsert`, `PaneRemoved`, `ServerStatus`, `NodeRemoved`); orchestration events (missed heartbeat, resync requested) are never published.
- Routing goes only to a currently `Online` node and fails immediately with `NodeUnreachable` otherwise (also `Stale`, `Disconnected`, unknown node). Nothing is queued or replayed: in-flight requests fail on disconnect and on timeout. Capabilities are checked against what the node accepted. Node-facing request ids are the orchestrator's own, mapped back to the UI's.
- `GetHostStatus` is dropped; per-server availability is carried by `ServerStatus` in snapshots and deltas.

## Identity, trust and enrollment (frozen before slice 4)

Concepts, kept separate:

| Concept | What it is |
|---|---|
| Identity | A long-lived asymmetric keypair with a self-signed certificate, generated once per node / orchestrator / UI. |
| `NodeId` = `HostId` | `sha256:<hex>` of the certificate's SubjectPublicKeyInfo. The fingerprint *is* the id; it is never stored twice. Same for the orchestrator's identity. |
| Trust | An explicit allowlist of identities the orchestrator currently authorizes (`trust.toml`). |
| Enrollment token | A random single-use secret with a short lifetime. It authorizes joining; it is not an identity and is not bound to a prospective `NodeId`. |
| Authentication | Mutual TLS 1.3 with self-signed certificates, verified by fingerprint, never by CA or hostname. |

Rules:

- On first join the node authenticates the orchestrator by a pinned expected fingerprint. Possession of an enrollment token never bypasses that check: a node refuses a server whose fingerprint differs, before sending the token.
- After joining, the node's fingerprint must be present and enabled in the orchestrator's trust store. Every connection is checked against it; an unknown, disabled or removed identity is refused.
- The hello's `node_id` must equal the fingerprint of the key that authenticated the connection (ADR "Identity" above); a mismatch is refused.
- A changed key under an existing display name is a different `NodeId` and is not trusted.

Enrollment flow:

1. `flight orchestrator enrollment create` yields `orchestrator=<fingerprint> address=<host:port> token=<secret> expires=<time>`; a copy/paste bundle may package them, the protocol keeps them independent.
2. The node connects over TLS and verifies the server fingerprint equals the expected one.
3. The node presents its client certificate (its public key) and the token in an `Enroll` request.
4. The orchestrator validates the token (hash lookup; invalid, expired or used is refused), authorizes the key (writes the trust store), and consumes the token atomically.

Tokens: 128-256 random bits, encoded base64url; single use; short lifetime (default 10 minutes); stored only as a SHA-256 hash; held in memory. An orchestrator restart discards outstanding tokens, which is an accepted v0 property. They never live in `trust.toml`.

`trust.toml` (public trust decisions only, operator-readable, version 1):

```toml
version = 1

[orchestrator]
fingerprint = "sha256:..."        # this orchestrator's identity (a node's copy pins it)
display_name = "flight-home"

[[nodes]]
id = "sha256:..."                 # the NodeId; no separate fingerprint field
display_name = "mini-1"
enabled = true                    # false = revoked, same as removing the entry
role = "node"                     # "node" (default) or "ui"
```

Revocation is "set `enabled = false` or delete the entry"; the orchestrator refuses identities it does not currently authorize. No CRLs. Private material lives apart, under `~/.config/flight/identity/` (`key.pem` mode 0600, `cert.pem`); `trust.toml` sits in `~/.config/flight/`.

Enrollment stays outside the replication protocol: it is its own unary RPC, the only call an authenticated-but-unauthorized identity may make.

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
2. `flight-node` core (done): `NodeCore` (state, Tracking, snapshot, semantic deltas), `NodeSession` (frame state machine: handshake, resync, routed requests), `TmuxServers` adapter. Snapshot + deltas = final snapshot is checked over 300 generated 60-round sequences.
3. `flight-orchestrator` core (done): registry, per-node cursors and transactional images, liveness, routing, fleet image and per-UI deltas. Properties checked over generated histories: UI mirrors always equal the fleet image; connected nodes converge to what they publish; an orchestrator rebuilt only from node snapshots reconstructs the live image.
4. Transport and pairing (done), on localhost:
   - `flight-trust`: `Identity` (rcgen self-signed key + cert, `key.pem` mode 0600), `Fingerprint` (`sha256:` of the SPKI = `HostId`), `TrustStore` (`trust.toml`, atomic save, roles `node`/`ui`), `EnrollmentTokens` (256-bit, single-use, hashed, in memory), `EnrollmentBundle` (copy/paste form), and the rustls verifiers: the client pins exactly one server fingerprint; the server requires a client certificate and proves possession of its key but leaves authorization to the application, so an unenrolled node can still reach `Enroll`.
   - `flight-transport`: a hand-bound tonic service (`flight.v1.Flight`: `NodeConnect`, `UiConnect`, `Enroll`; no codegen, paths checked against the `.proto`) over TLS 1.3 accepted by our own listener, so the peer's fingerprint travels with every request. `serve()` runs the `OrchestratorCore` behind one lock (never held across an await), re-checks trust every tick so revocation drops live connections, and ends streams at shutdown. `NodeLink` redials with backoff and holds the `NodeSession`; `UiClient` and `enroll()` are the other two clients.
   - Verified on localhost with real sockets: hello, snapshot, deltas, preview, disconnect, reconnect and resync; unknown, disabled, removed, wrong-role and forged-hello identities refused; enrollment once, expired, replayed and against a wrong pinned fingerprint (the token is not spent).
5. Operate it (done):
   - CLI roles: `flight orchestrator run | enrollment create | trust list/revoke/status`, `flight node join | run`, `flight ui join | run`. The bare `flight` is still the direct local/SSH dashboard. Each role has its own directory and identity under the config dir (`orchestrator/`, `node/`, `ui/`; `identity/`, `trust.toml`, `connection.toml`, `admin.sock`). Enrollment tokens live in the orchestrator's memory, so `enrollment create` is a request to the running process over an owner-only Unix socket in its private directory.
   - `join` is all-or-nothing: bundle expiry is checked first, the orchestrator is authenticated by the pinned fingerprint before the token is sent, the reply must name this identity and that orchestrator, and the orchestrator must really accept the stream; only then are identity and settings written (atomically, and a newly made identity is removed if the write fails). A failed join leaves no files and, for a wrong pin, does not spend the token.
   - The node is the observation service: `TmuxServers` polled on a blocking thread feeds `NodeCore` (classify, fuse, resolve, Tracking) and streams snapshot and deltas through `NodeLink`; preview and kill are control jobs.
   - The dashboard reads through a `Backend` trait with two implementations: the direct `Collector` and `flight-client`'s `OrchestratedBackend` (`UiClient` -> `FleetImage` -> `UiSnapshot`). It is configured with an endpoint and a pinned identity only. Hosts carry a display label; identity stays the node id. Liveness shows as `stale`/`disconnected (last known)`, and a dead orchestrator link is an explicit row above last-known nodes.
   - Differential test: the same scripted tmux server feeds the direct collector and node -> orchestrator -> UI over real mTLS; their dashboard data must agree at every step (detection, state change, Done, pane replaced, tmux gone), as must previews. A real-process test enrolls a node and a UI by bundle and shows agents through the orchestrator.
   - Not yet: switching to a remote pane through the orchestrator (the dashboard says so), hooks and process discovery on the node.
6. Real second machine on the LAN.
7. Measure, then decide on packaging and the next UX changes.

## Backpressure and off-lock control (before slice 5)

Invariant: **the replication backlog is bounded; falling behind causes resynchronization, never unbounded buffering.** Every outbound queue is a bounded `Outbox` with three classes: deltas are disposable (on overflow they are all discarded, further deltas dropped, and a fresh snapshot is queued under the lock that orders snapshots against deltas, which restarts the sequence); reliable items (responses, snapshots, hellos, requests) are bounded and a peer that will not read them is dropped; heartbeats and resync requests coalesce to at most one queued. One item at a time reaches the gRPC stream, so HTTP/2 flow control reaches the outbox. `ServerHandle::max_backlog()` is the gauge.

The session lock protects state transitions, not I/O. `NodeSession` validates a control request under its state (handshake, capability, the pane is published) and returns a `ControlJob` capturing the target; the transport runs it on a blocking thread, at most four at once (excess is refused at once as busy, each has a ten second limit), and queues the response. Replication, heartbeats, revocation and shutdown never wait on tmux. Destructive jobs carry the pid published for the pane: `kill_pane` refuses unless the pane id still belongs to that process, so a reused `%id` is never killed by a request meant for its predecessor.

## Known limits after slice 4
- Key rotation and certificate renewal are not designed (a new key is a new identity; re-enroll).
- The next milestone is the real LAN experiment: orchestrator + node on one Mac, a node on another, the dashboard on a laptop, with measurements (state-change and preview latency, idle CPU and traffic, reconnection, network loss, restarts of each role, 10/50/100 simulated panes) before any optimization.

## Open questions

1. Hook ingestion path on the node (slice 2+).
