// SPDX-License-Identifier: MIT

//! Creating a session on this node: validate, resolve, launch, with nothing left behind on
//! failure. The node owns all of it; the orchestrator only routes the request here.

mod create;
mod new_id;
mod session_env;
mod session_request;
mod surface_request;

pub use session_env::SessionEnv;
pub use session_request::{Program, SessionRequest};
pub use surface_request::{NewSurface, SurfaceRequest};

pub(crate) use create::create;
pub(crate) use new_id::new_id;
