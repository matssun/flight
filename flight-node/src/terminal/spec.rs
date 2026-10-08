// SPDX-License-Identifier: MIT

use flight_proto::TERMINAL_ID_LEN;
use flight_state::{PaneId, ServerId};

/// A validated request to open a terminal, detached from the session. Everything in it was
/// checked at the boundary; nothing here came from a peer as a string that is later executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSpec {
    pub request_id: u64,
    pub terminal_id: [u8; TERMINAL_ID_LEN],
    pub server: ServerId,
    pub pane: PaneId,
    /// The pane process the caller saw.
    pub pid: u32,
    pub cols: u16,
    pub rows: u16,
    pub term: String,
}
