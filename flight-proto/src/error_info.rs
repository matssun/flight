// SPDX-License-Identifier: MIT

use crate::{ErrorKindCode, Reject, Validate};

#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct ErrorInfo {
    #[prost(enumeration = "ErrorKindCode", tag = "1")]
    pub kind: i32,
    #[prost(string, tag = "2")]
    pub message: String,
}

impl Validate for ErrorInfo {
    fn validate(&self) -> Result<(), Reject> {
        ErrorKindCode::decode(self.kind, "error_info.kind").map(|_| ())
    }
}
