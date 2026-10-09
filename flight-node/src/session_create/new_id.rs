// SPDX-License-Identifier: MIT

use crate::ControlError;
use flight_proto::ErrorKindCode;

/// A fresh random id such as `w-3f9c1a2b4d5e6f70`: 64 random bits from the operating system.
/// Ids identify; they carry no meaning, and no name or backend id is folded into them.
pub(crate) fn new_id(prefix: char) -> Result<String, ControlError> {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).map_err(|_| {
        ControlError::new(
            ErrorKindCode::RemoteCommandFailed,
            "this node cannot make a new id",
        )
    })?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{prefix}-{hex}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_valid_distinct_and_prefixed() {
        let (a, b) = (new_id('w').unwrap(), new_id('w').unwrap());
        assert_ne!(a, b);
        assert!(a.starts_with("w-") && flight_state::valid_id(&a));
    }
}
