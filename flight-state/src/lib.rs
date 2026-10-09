// SPDX-License-Identifier: MIT

//! flight-state — typed identity and agent-state policy. Pure: no I/O, and no knowledge of
//! how a state was observed (tmux text, process status, SSH, a future daemon).
//! See docs/adr/ADR-001-architecture.md.

#[macro_use]
mod macros;

mod agent_state;
mod host_id;
mod pane_id;
mod pane_ref;
mod placement;
mod server_id;
mod session_id;
mod session_name;
mod session_ref;
mod surface_id;
mod surface_role;
mod window_id;
mod workspace_id;

pub mod policy;

pub use agent_state::AgentState;
pub use host_id::HostId;
pub use pane_id::PaneId;
pub use pane_ref::PaneRef;
pub use placement::RawPlacement;
pub use policy::{needs_attention, sort_rank};
pub use server_id::ServerId;
pub use session_id::SessionId;
pub use session_name::{
    valid_dir, valid_session_name, MAX_DIR_LEN, MAX_SESSION_NAME_LEN, RESERVED_VIEW_PREFIX,
};
pub use session_ref::SessionRef;
pub use surface_id::SurfaceId;
pub use surface_role::SurfaceRole;
pub use window_id::WindowId;
pub use workspace_id::{valid_id, WorkspaceId, MAX_ID_LEN};

#[cfg(test)]
mod identity_tests;
