// SPDX-License-Identifier: MIT

/// A transport connection from a node. Opaque: supplied by whatever carries the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnId(pub u64);

/// A connected UI client. Opaque, like [`ConnId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UiId(pub u64);
