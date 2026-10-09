// SPDX-License-Identifier: MIT

use tokio::sync::mpsc;

/// The user's side of a terminal: what they type, how big the window is, and where output goes.
pub struct LocalTerminal {
    pub input: mpsc::Receiver<Vec<u8>>,
    pub resizes: mpsc::Receiver<(u16, u16)>,
    pub output: mpsc::Sender<Vec<u8>>,
}
