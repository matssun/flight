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
- `RevealPane` (was `SwitchPane`; see ADR-003), `SendInput`, `KillPane`, `CreateSession { name, dir, command? }`, `KillServer`, `GetHostStatus`
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
   - Not yet: switching to a pane through the orchestrator (design in ADR-003; the node-side guarded reveal exists, the dashboard path does not), hooks and process discovery on the node.
6. Real second machine on the LAN.
7. Measure, then decide on packaging and the next UX changes.

## Backpressure and off-lock control (before slice 5)

Invariant: **the replication backlog is bounded; falling behind causes resynchronization, never unbounded buffering.** Every outbound queue is a bounded `Outbox` with three classes: deltas are disposable (on overflow they are all discarded, further deltas dropped, and a fresh snapshot is queued under the lock that orders snapshots against deltas, which restarts the sequence); reliable items (responses, snapshots, hellos, requests) are bounded and a peer that will not read them is dropped; heartbeats and resync requests coalesce to at most one queued. One item at a time reaches the gRPC stream, so HTTP/2 flow control reaches the outbox. `ServerHandle::max_backlog()` is the gauge.

The session lock protects state transitions, not I/O. `NodeSession` validates a control request under its state (handshake, capability, the pane is published) and returns a `ControlJob` capturing the target; the transport runs it on a blocking thread, at most four at once (excess is refused at once as busy, each has a ten second limit), and queues the response. Replication, heartbeats, revocation and shutdown never wait on tmux. Destructive jobs carry the pid published for the pane: `kill_pane` refuses unless the pane id still belongs to that process, so a reused `%id` is never killed by a request meant for its predecessor.

## Known limits after slice 4
- Key rotation and certificate renewal are not designed (a new key is a new identity; re-enroll).
- The next milestone is the real LAN experiment: orchestrator + node on one Mac, a node on another, the dashboard on a laptop, with measurements (state-change and preview latency, idle CPU and traffic, reconnection, network loss, restarts of each role, 10/50/100 simulated panes) before any optimization.

## LAN experiment (first results)

Setup: two Apple-silicon Macs on one LAN (release builds). Mac A ran the orchestrator, a node and the UI; Mac B ran a node. Real tmux panes, mutual TLS between the machines. Measurement tooling is a separate crate (`tools/flight-load`: `lan`, `watch`, `synth`), never a mode of `flight node run`. Not yet covered: a real network (Wi-Fi/cable) interruption, and a three-machine layout.

**Latency (state change on a node -> visible in a UI image).** The node polls every 2 s, so latency is the poll phase plus about 50 ms: local node min 52 ms, p50 1.27 s, max 2.0 s; the LAN node (trigger over ssh, so +-0.2 s) p50 1.35-1.6 s, max about 2.0-2.5 s. The network adds little; poll cadence dominates. (A first run looked like a constant 1.88 s: the measurement was phase-locked to the poll interval. Triggers now use random gaps.) Preview of the selected pane: 3-5 ms from a local node, 16-21 ms (p50 17) across the LAN.

**Control plane with synthetic panes** (`flight-load synth`: a real node identity, `NodeLink` and `NodeCore` including classification, scripted panes, loopback; flips every 500 ms):

| panes | flips/s | flip -> UI p50 / p99 | orchestrator CPU | orchestrator RSS | UI stream |
|---|---|---|---|---|---|
| 10 | 4 | 0.3 / 0.5 ms | 0.09% | 6.5 MB | 0.8 KB/s |
| 50 | 10 | 0.4 / 1.2 ms | 0.07% | 6.4 MB | 2.1 KB/s |
| 100 | 20 | 0.6 / 1.3 ms | 0.07% | 7.0 MB | 4.0 KB/s |
| 500 | 50 | 2.0 / 128 ms | 0.15% | 7.3 MB | 11.6 KB/s |

Every flip was seen, no resyncs, `backlog=0` throughout. The 500-pane p99 is the initial snapshot burst. The orchestrator is not the scaling limit.

**Real tmux on the node** (100 panes on one node, default 2 s poll; *CPU figures here and in the polling sweep below omit the short-lived `tmux` processes the node spawns and are superseded by "Observation strategy" below*): the node used 0.8% of a core and 7.5 MB, its tmux server another 1.5-1.75%: polling costs roughly 0.008% of a core per pane for the node, plus the tmux server's share. Idle with 3 panes: node 0.1% CPU, 7 MB; orchestrator 0.01%, 5.8 MB; about 20 B/s out and 9 B/s in per node (heartbeats only). With panes changing: about 170 B/s per node, about 320 B/s into the UI, about 200-235 B per state change on the UI stream. The TUI itself: about 0.9% CPU, 8 MB.

**Failure and recovery** (timeline from `flight-load watch`):

| event | observed |
|---|---|
| orchestrator killed | UI link lost at once; agents and tmux unaffected |
| orchestrator restarted | UI relinked in 0.07 s; nodes reconnected at +4.4 s and +5.7 s (their backoff after 20 s of failures); fleet rebuilt with panes and Done state intact |
| node killed | `Disconnected` in 0.14 s, last-known panes kept |
| node restarted | `Online` and converged in 0.26 s |
| node frozen (silent partition) | `Stale` at 15.7 s, `Disconnected` at 20 s, `Online` 0.6 s after resume |
| tmux server killed on a node | its panes left the UI within 1.4 s; back within 1.35 s of restart |
| node revoked while connected | `Disconnected` in 11 ms; stays refused (`refused: not authorized`) |

**Polling sweep (real tmux, this Mac; node CPU % / tmux-server CPU % of one core, orchestrator under 0.05% throughout).** Panes are real tmux panes running a stand-in agent; the node polls with `list-panes` plus one `capture-pane` per agent pane. Measured with the original schedule (sleep the full interval after each round):

| panes | 0.25 s | 0.5 s | 1 s | 2 s | 5 s |
|---|---|---|---|---|---|
| 10 | 1.4 / 0.7 | 0.7 / 0.4 | 0.4 / 0.2 | 0.2 / 0.1 | 0.1 / 0.03 |
| 50 | 3.5 / 3.0 | 2.2 / 1.9 | 1.3 / 1.1 | 0.8 / 0.6 | 0.3 / 0.2 |
| 100 | 5.2 / 6.2 | 3.5 / 4.4 | 2.4 / 3.0 | 1.3 / 1.5 | 0.6 / 0.7 |
| 250 | 6.5 / 15 | 5.3 / 12 | 3.9 / 9.1 | 2.7 / 6.2 | 1.3 / 3.0 |
| 480 | 6.6 / 28 | 6.2 / 26 | 5.1 / 21 | 3.7 / 16 | 1.8 / 7.9 |

State-change latency at 100 panes (p50 / p90 / max, ms; trigger to visible): 0.25 s: 746/772/783, 0.5 s: 933/1125/1131, 1 s: 1429/2047/3374, 2 s: 2153/2540/2565, 5 s: 4116/4946/5010.

What this shows:
- **The cost is in tmux, not in Flight or the orchestrator.** The tmux server uses as much CPU as the node at 100 panes and about four times as much at 480 (one `capture-pane` subprocess per pane per round). The orchestrator is irrelevant.
- **A polling round takes about 3.5-4.2 ms per pane, sequentially**: 59 ms at 10 panes, 173 ms at 50, 358 ms at 100, 880 ms at 250, about 2 s at 480. Detection latency is therefore poll interval + round time, and above roughly 100 panes the interval cannot help: 480 panes at a 1 s interval still show a median of about 3.9 s.
- The first schedule slept the whole interval after each round, so the real period was interval + round (a "1 s" poll at 100 panes behaved like 1.4 s).

Decisions taken on this evidence: rounds now start about one interval apart, with at least as much idle time as the previous round's duration (period = max(interval, 2 x round)), so tmux is never driven back to back; a round slower than the interval is reported in the node log; the default interval is 1 s (was 2 s): with up to 50 panes it costs about 2% of a core in total and cuts the typical latency from about 1.2 s to about 0.6 s. Re-measured with the new schedule: 100 panes at 1 s: latency 874/1251/1321 ms at 3.0% + 3.5% CPU (was 1429/2047/3374 ms), 100 panes at 2 s: 1638/2391/2831 ms (was 2153/2540/2565); 480 panes at 1 s: 3876/5116/5814 ms at 4.2% + 16% CPU.

Not done, with the measured justification for the next step when pane counts grow: capture panes concurrently (a few at a time) or through one persistent tmux control-mode connection instead of a subprocess per pane, and skip panes that provably did not change. Not needed below about 100 panes.

**Control plane, larger and harsher (synthetic panes, loopback, one UI; 500 ms ticks):**

| scenario | result |
|---|---|
| 1000 panes, 50 flips per tick | all 4450 flips seen, flip -> UI p50 4.1 / p99 5.2 ms; orchestrator 0.25% CPU, 9.5 MB; backlog 0; about 24 KB/s to the UI |
| 500 panes, every pane flips every tick (44,500 flips in 45 s) | all seen, p50 4.7 / p99 24.7 ms; orchestrator 0.39% CPU, 10.6 MB; backlog 0; the node discards queued deltas and sends snapshots, so the UI received 92 events and about 300 KB/s: overload degrades into snapshots, never into unbounded queues |
| orchestrator killed for 6 s and restarted with 500 panes | the whole fleet was visible in the UI again 1.5 s after the orchestrator returned (node and UI reconnects included) |

Findings and decisions:
- The 20 s (not 30 s) disconnect on a frozen node is the server's HTTP/2 keepalive (10 s interval, 10 s timeout) detecting the dead peer before the heartbeat rule; both stay.
- A revoked or unreachable node used to retry silently. `NodeLink` now takes a log hook and `flight node run` prints `connected to ...`, `link ended after ...` and `link down after ...: <why>; retrying` (identical failures once).
- Nodes that are gone for good stay in the orchestrator image as `Disconnected` indefinitely (the `NodeRemoved` pruning policy is still open); after the scaling runs the UI listed hundreds of ghost synthetic panes until the orchestrator was restarted.
- The two sites are joined by a route-based IPsec tunnel whose path MTU is 1419 while hosts assumed 1500 (MTU/MSS on Auto, and an overlap with a Site Magic tunnel prevented saving a fix). Large TCP packets stalled across it (long-standing intermittent SSH hangs; an 11 MB scp stalled; Flight's TLS handshake stalled the same way). On one LAN none of this applies. Flight itself does not work around a black-holed path MTU; the fix is MSS clamping on the tunnel.
- **Physical interruption (Wi-Fi off 40 s, then on, on the node host): a node started before the interruption did not recover, reproducibly (6 of 6 runs).** The orchestrator side behaved: `Stale` about 5 s and `Disconnected` about 9 s after the cut, last-known panes kept, control requests answered at once. Once the interface was back (reachable by ping and `nc`, ARP valid) the node failed every dial for as long as it was watched (minutes) with `No route to host` (EHOSTUNREACH in microseconds, no packet sent). Bounded diagnostics on the same host, all comparing the same orchestrator by IPv4 literal, by hostname (IPv6 and IPv4 results) and by IPv6 link-local:
  - the same Rust binary started from a **new ssh session** reached it at once (IPv4 connected, IPv6 refused = routable);
  - the same Rust binary started **inside the tmux server that predates the bounce** failed (including copies spawned by a stuck probe and restarts by a shell loop living in that tmux server);
  - a long-lived **Python** process in that same tmux server connected normally; plain `std::net` and tokio Rust probes failed identically, so it is not Flight, tonic or tokio;
  - in one stuck Rust process the LAN router and a public address were reachable while the orchestrator host was not (not Local Network privacy; not general connectivity loss).

  Narrowly: on the tested macOS, Rust sockets created by processes launched inside a tmux server/session that predates a Wi-Fi interface bounce can stay unable to route to the prior LAN peer after connectivity returns, while Python in the same tmux environment, and the same Rust binary launched from a fresh login context, succeed. This does not establish tmux itself as the cause; the underlying cause is probably macOS per-session or per-process network state. Investigation stopped here. Flight must not try to repair the host (no route flushing, interface cycling or TCC changes).

  Consequences: (1) every dial is now bounded (TCP and TLS 5 s, stream accept 10 s) and the client sends HTTP/2 keepalives, so a half-open connection is detected; (2) `flight node run --exit-after-link-down SECS` (off by default) exits with status 75 after an unbroken window of only immediate no-route failures (jittered by up to 25%, never because the orchestrator is merely down, refusing or slow). This is safe because a restarted node gets a new incarnation and a full snapshot while tmux and the agents are untouched; (3) the restart only helps if the restarter is **outside** the affected session, i.e. launchd/systemd (init-parented), not a tmux or ssh shell loop, which was shown to stay stuck; launchd itself is not yet tested here; (4) `flight node run` is documented as a service process: run it from launchd/systemd, not from a login shell, ssh or tmux (`contrib/launchd/`: a LaunchAgent template, install steps, and the recovery experiment). `--exit-after-link-down` stays opt-in until a launchd restart is shown to cure the failure (target: 5 of 5 toggles, with the service pid before and after); only then decide whether it becomes the macOS default. No internal supervisor is built.
- **Service-manager experiment (node run by a launchd LaunchAgent, `--exit-after-link-down 60`, same Wi-Fi toggle: off 40 s, then on): the failure did not occur, 6 of 6 in the initial run set (one hand-timed, five scripted) and 8 of 8 in total after two further runs following the tmux locale fix below.** The same service process (pid 197, `runs = 1`, never exited, no "exiting to be restarted" line) reconnected on its own 4-11 s after the interface returned; the orchestrator showed `Stale` at about 15 s, then `Online` again at 44-54 s after the toggle started. Under launchd the stream was not torn down at the moment the interface dropped (the orchestrator noticed by heartbeat silence, about 15 s, where the tmux-launched node was seen `Stale` at about 5 s); the node noticed its end later and reconnected cleanly. A tmux-launched node never recovered in 6 of 6 identical runs. This supports the narrower claim that the failure is tied to the launch context (a tmux/ssh session that predates the bounce), and that the documented deployment model avoids it.
- **The exit-and-restart path under launchd** (one run, Wi-Fi off 100 s): the node exited after 65 s of continuous no-route failures, `last exit code = 75: EX_TEMPFAIL`, launchd started a fresh process (pid 197 -> 8249, `runs = 2`) while Wi-Fi was still off, which connected 8 s after Wi-Fi returned; the orchestrator showed `Online` and a new incarnation resynchronised. Note this also fires for a genuine long outage of the machine's own network (immediate no-route errors are exactly what a switched-off Wi-Fi produces), so a laptop node without network restarts about once a minute; that is harmless (tmux and agents are untouched) but noisy. Because launchd-run nodes recovered without it, the exit path has not been shown to cure a stuck process (none occurred); it stays opt-in as a safety net.
- **A node run without a UTF-8 locale saw no panes at all (found because the launchd service reported `online, panes=0` while 100 agent panes existed).** Without `LANG`/`LC_*`, which is what launchd, systemd and non-login ssh commands provide, tmux rewrites the tab separators in `-F` output (and non-ASCII characters in captured screens) to `_`; the pane parser skipped every line and the node reported an empty server. A manual node from an ssh session did not show it (the login environment has a UTF-8 locale), which is why every earlier test passed. Fixed: every tmux call (local runner and SSH runner) now passes `-u`, and a pane listing in which tmux returned lines but none parsed is an error ("tmux output was not in the expected format... is its locale UTF-8?"), never "no panes". Regression tests: argument order, the unparseable case, and a live test that runs real tmux with an empty environment. The earlier launchd Wi-Fi experiments are unaffected (liveness and reconnection do not depend on the pane count), but the service node in them reported zero panes.
- **Where the stale/disconnected timing comes from (instrumented, node under launchd, Wi-Fi off):** the orchestrator log reads `Stale, no frame for 16s (stale after 15s; heartbeat every 5s)` and, 4 s later, `connection ended, last frame 20s ago`. `Stale` is therefore the heartbeat rule (3 x 5 s, seen on the 1 s tick) and `Disconnected` at about 20 s is the server's HTTP/2 keepalive (10 s interval + 10 s timeout) beating the 30 s heartbeat limit; both measured consistently (14-16 s and 19-20 s) over repeated toggles. The shorter stale times seen earlier came from a tmux-launched node and from runs where the marker lagged the real cut. No threshold is changed on this evidence; the log lines make any future difference visible.
- The node host's IP changed across the bounce because its Wi-Fi uses a private (randomized) MAC address that does not match the router's fixed-IP reservation. Not caused by Flight, but it matters for operators: nodes should be reachable by identity, never by address, and the orchestrator address in `connection.toml` must be stable (a name or a reservation).

Open from the experiment: a repeatable physical-interruption test and the node recovery bug above, UI on a third machine, 500+ real panes, and whether the 2 s default poll should change (latency is poll-bound; CPU is far below any concern).

## Open questions

1. Hook ingestion path on the node (slice 2+).

## Observation strategy (supersedes the CPU figures and the 1 s default above)

The polling figures above counted the long-lived node and tmux-server processes but not the `tmux capture-pane` processes the node spawns each round. Counted properly, the sequential observer costs about 29% of a core at 100 panes and a 1 s interval (about 57% + 13% at 250 panes), not 3%. The 2 s to 1 s default change was decided on the incomplete figure.

Measured on this Mac with real tmux panes (250 panes, 20 s runs, no pane stale at the end in any run listed; CPU = client + tmux server, % of a core; full tables in `docs/STATUS.md`):

| Strategy | Round at 250 panes | CPU, 5% churn, 1 s | CPU, 50% churn, 1 s |
|---|---|---|---|
| sequential, one process per capture | 680 ms | 57 + 13 | 58 + 15 |
| 8 captures at once | 137 ms | 81 + 11 | 80 + 14 |
| control mode, capture all | 21 ms | 1.1 + 2.9 | 1.3 + 6.9 |
| control mode, skip unchanged | 2 ms | 0.3 + 0.6 | 0.7 + 4.8 |

Decisions:

- **Control mode with skipping is the node's default observer** (`flight node run --observer ctl-skip`), with the sequential observer kept as the reference path (`--observer seq`), the fallback, and the oracle the tests compare against. `flight-node` consumes a `PaneObserver`; control-mode parsing lives in `flight-tmux` and never reaches `NodeCore`.
- **The default interval is 500 ms**: at 250 panes the control-mode observer costs 2.2% of a core at 5% churn and 8.1% at 50%, and the detection latency p50/p95 drops from about 525/975 ms to 280/500 ms.
- **Skipping is conservative.** A screen is reused only when the pane is known, has the same pid, command and title, tmux reports window activity strictly before the second of the last capture, and the capture is younger than 30 s. Unknown or equal activity, a clock that went backwards, or anything else uncertain is captured.
- **Resynchronisation dominates optimisation.** Any control-connection error (closed, `%exit`, a reply that does not match its command, no reply in 10 s, an unparseable pane list, a missing own client) drops the connection and every cached screen. That round is answered by the sequential path, the connection is re-established (backing off 5 s after a failed attempt), and the first round after that captures everything. Both transitions are logged as operator notes.
- **Event-driven capture was tried and rejected.** tmux sends `%output` to a control client only for panes of the session it is attached to, so it cannot be a fleet-wide change signal; an early result that seemed to show otherwise was an invalid measurement (it appeared only when a previous run had been made on the same server).
- **The control client is one attached client of one session.** tmux counts it in `#{session_attached}`; the observer finds its own client by pid and discounts it, so it never makes a pane look focused. A person attached to the same session still counts.
- Real panes are capped by the host's pty limit (macOS `kern.tty.ptmx_max` is 511), which is a host limit, not a Flight one; synthetic node state scales past 1000.
