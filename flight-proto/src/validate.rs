// SPDX-License-Identifier: MIT

use crate::Reject;

/// Semantic validation of a decoded message. Decoding only proves the bytes parse; this proves
/// the message means something this build understands.
pub trait Validate {
    fn validate(&self) -> Result<(), Reject>;
}

pub(crate) fn non_empty(value: &str, what: &'static str) -> Result<(), Reject> {
    if value.is_empty() {
        Err(Reject::Empty(what))
    } else {
        Ok(())
    }
}
