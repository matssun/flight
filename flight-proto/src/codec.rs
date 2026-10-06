// SPDX-License-Identifier: MIT

//! Encoding and checked decoding. Decoding never panics on peer bytes: it returns a
//! [`Reject`], and a decoded message is also validated before the caller sees it.

use crate::{Reject, Validate};
use prost::Message;

/// The largest frame accepted. Snapshots of a few hundred panes are far below this.
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

pub fn encode<M: Message>(message: &M) -> Vec<u8> {
    message.encode_to_vec()
}

/// Decode and validate. Unknown fields are ignored; unknown enum values are rejected.
pub fn decode<M: Message + Default + Validate>(bytes: &[u8]) -> Result<M, Reject> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(Reject::TooLarge {
            len: bytes.len(),
            max: MAX_FRAME_BYTES,
        });
    }
    let message = M::decode(bytes).map_err(|e| Reject::Malformed(e.to_string()))?;
    message.validate()?;
    Ok(message)
}
