// SPDX-License-Identifier: MIT

use flight_proto::Incarnation;

/// A fresh random incarnation for this node process. Never persisted: a restarted node gets
/// a new one, which is how receivers tell a restart from a continuation.
pub fn fresh_incarnation() -> Result<Incarnation, getrandom::Error> {
    let mut bytes = [0u8; Incarnation::LEN];
    getrandom::getrandom(&mut bytes)?;
    Ok(Incarnation::from_bytes(bytes))
}
