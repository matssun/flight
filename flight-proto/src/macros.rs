// SPDX-License-Identifier: MIT

/// Defines a wire enum whose `0` is always `Unspecified`. Decoding rejects both `Unspecified`
/// and values this build does not know, so a newer peer's new variant is refused, not guessed.
macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $n:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, prost::Enumeration)]
        #[repr(i32)]
        pub enum $name {
            Unspecified = 0,
            $($variant = $n),+
        }

        impl $name {
            pub(crate) fn decode(value: i32, field: &'static str) -> Result<Self, crate::Reject> {
                match Self::try_from(value) {
                    Ok(Self::Unspecified) | Err(_) => {
                        Err(crate::Reject::UnknownEnum { field, value })
                    }
                    Ok(known) => Ok(known),
                }
            }
        }
    };
}
