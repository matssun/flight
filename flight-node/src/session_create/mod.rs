// SPDX-License-Identifier: MIT

//! Creating a session on this node: validate, resolve, launch, with nothing left behind on
//! failure. The node owns all of it; the orchestrator only routes the request here.

mod create;
mod session_env;
mod session_request;

pub use session_env::SessionEnv;
pub use session_request::{Program, SessionRequest};

pub(crate) use create::create;
