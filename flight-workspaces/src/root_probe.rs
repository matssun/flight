// SPDX-License-Identifier: MIT

use crate::RootState;

/// Looks at a root on the host that owns it. The local implementation is [`crate::FsProbe`]; a
/// remote host is asked through its node, and a node that cannot be reached answers
/// `HostUnreachable`. A probe only reads: it never creates, repairs or deletes anything.
pub trait RootProbe {
    fn probe(&self, host: &str, path: &str) -> RootState;
}
