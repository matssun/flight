// SPDX-License-Identifier: MIT

//! The Flight wire protocol: pure types, versioning, validation and the replication cursor.
//! No sockets, no TLS, no tmux, no async. The messages are hand-written `prost` types so no
//! `protoc` is needed at build time; they plug into tonic's `ProstCodec` later.
//!
//! The wire carries semantic pane/node state only. Classifier internals (hook files, screen
//! matches, glyph anchors) never appear here.
//!
//! Compatibility rules:
//! - protocol major mismatch: reject the connection ([`ProtocolVersion::negotiate`]);
//! - unknown optional field: ignored (protobuf);
//! - unknown capability: negotiated away ([`capability::negotiate`]);
//! - unknown or unspecified enum value: reject that message, never guess ([`Validate`]).

#[macro_use]
mod macros;

mod codes;
mod command;
mod delta;
mod enroll;
mod error_info;
mod fleet;
mod fleet_image;
mod heartbeat;
mod hello;
mod incarnation;
mod node_frame;
mod orchestrator_frame;
mod pane_ref_msg;
mod pane_state;
mod reject;
mod replication;
mod response;
mod server_status;
mod snapshot;
mod terminal;
mod ui_frame;
mod validate;
mod version;

pub mod capability;
pub mod codec;

pub use codes::{
    AgentKindCode, ErrorKindCode, ExitReasonCode, NodeStatusCode, RoleCode, SourceCode, StateCode,
};
pub use command::{command_kind, Command, Request, MAX_PREVIEW_LINES};
pub use delta::{delta_change, Delta};
pub use enroll::{EnrollRequest, EnrollResponse};
pub use error_info::ErrorInfo;
pub use fleet::{
    fleet_change, FleetDelta, FleetSnapshot, NodeRemoved, NodeServerStatus, NodeStatusChanged,
    NodeView,
};
pub use fleet_image::{FleetImage, FleetNode};
pub use heartbeat::Heartbeat;
pub use hello::{NodeHello, OrchestratorHello};
pub use incarnation::Incarnation;
pub use node_frame::{node_body, NodeFrame};
pub use orchestrator_frame::{orchestrator_body, Goodbye, OrchestratorFrame, ResyncRequest};
pub use pane_ref_msg::PaneRefMsg;
pub use pane_state::PaneState;
pub use reject::Reject;
pub use replication::{ReplicationCursor, Step};
pub use response::{response_result, Preview, Response};
pub use server_status::{AvailabilityCode, ServerStatus};
pub use snapshot::Snapshot;
pub use terminal::{
    terminal_body, valid_term, Origin, TerminalAttach, TerminalClose, TerminalData, TerminalExit,
    TerminalFrame, TerminalOpened, TerminalResize, MAX_TERMINAL_DATA, MAX_TERMINAL_DIM,
    MAX_TERM_LEN, TERMINAL_ID_LEN,
};
pub use ui_frame::{ui_event_body, ui_request_body, Subscribe, UiEvent, UiRequest};
pub use validate::Validate;
pub use version::{ProtocolVersion, CURRENT_VERSION};
