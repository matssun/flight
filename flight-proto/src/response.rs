// SPDX-License-Identifier: MIT

use crate::{ErrorInfo, Reject, Validate};

#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Preview {
    #[prost(string, tag = "1")]
    pub text: String,
    /// Seconds since the epoch at which the node captured it.
    #[prost(uint64, tag = "2")]
    pub captured_at: u64,
}

/// The outcome of one [`crate::Request`], matched by `request_id`.
#[derive(Clone, PartialEq, Eq, prost::Message)]
pub struct Response {
    #[prost(uint64, tag = "1")]
    pub request_id: u64,
    #[prost(oneof = "response_result::Result", tags = "2, 3, 4, 5")]
    pub result: Option<response_result::Result>,
}

pub mod response_result {
    use super::{ErrorInfo, Preview};
    use crate::TerminalOpened;

    /// The request succeeded and has no payload.
    #[derive(Clone, Copy, PartialEq, Eq, prost::Message)]
    pub struct Done {}

    #[derive(Clone, PartialEq, Eq, prost::Oneof)]
    pub enum Result {
        #[prost(message, tag = "2")]
        Done(Done),
        #[prost(message, tag = "3")]
        Preview(Preview),
        #[prost(message, tag = "4")]
        Error(ErrorInfo),
        #[prost(message, tag = "5")]
        Terminal(TerminalOpened),
    }
}

impl Validate for Response {
    fn validate(&self) -> Result<(), Reject> {
        match self
            .result
            .as_ref()
            .ok_or(Reject::Missing("response.result"))?
        {
            response_result::Result::Error(e) => e.validate(),
            response_result::Result::Terminal(t) => {
                if t.terminal_id.len() == crate::TERMINAL_ID_LEN {
                    Ok(())
                } else {
                    Err(Reject::OutOfRange("terminal_opened.terminal_id"))
                }
            }
            response_result::Result::Done(_) | response_result::Result::Preview(_) => Ok(()),
        }
    }
}
