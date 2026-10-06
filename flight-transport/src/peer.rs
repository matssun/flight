// SPDX-License-Identifier: MIT

use flight_trust::Fingerprint;

/// The authenticated identity of the TLS peer behind a request: the fingerprint of the key
/// it proved possession of in the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity(pub Fingerprint);
