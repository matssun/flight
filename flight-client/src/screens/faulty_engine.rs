// SPDX-License-Identifier: MIT

//! A test double for the one thing the real emulator will not do on demand: fail. It is the real
//! engine, except that bytes containing [`FAIL`] fail the way a parser panic does (the screen is
//! emptied, the failure is reported). Unit tests of this crate use it as the engine, so a
//! failing emulator can be tested all the way up through a presentation session.

use super::engine::TerminalEngine;
use super::{CellView, EngineFailure, Geometry, Modes};

/// What makes the engine fail when it is fed.
pub(crate) const FAIL: &[u8] = b"\x1b[!fail!";

pub(super) struct FaultyEngine<E> {
    inner: E,
}

impl<E: TerminalEngine> TerminalEngine for FaultyEngine<E> {
    fn new(geometry: Geometry) -> Self {
        Self {
            inner: E::new(geometry),
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<(), EngineFailure> {
        if bytes.windows(FAIL.len()).any(|w| w == FAIL) {
            let geometry = self.inner.size();
            self.inner = E::new(geometry);
            return Err(EngineFailure {
                message: "injected failure".to_owned(),
                bytes: bytes.len(),
                geometry,
            });
        }
        self.inner.feed(bytes)
    }

    fn resize(&mut self, geometry: Geometry) {
        self.inner.resize(geometry);
    }

    fn size(&self) -> Geometry {
        self.inner.size()
    }

    fn cell(&self, col: u16, row: u16) -> Option<CellView<'_>> {
        self.inner.cell(col, row)
    }

    fn cursor(&self) -> Option<(u16, u16)> {
        self.inner.cursor()
    }

    fn modes(&self) -> Modes {
        self.inner.modes()
    }
}
