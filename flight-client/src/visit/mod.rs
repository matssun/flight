// SPDX-License-Identifier: MIT

//! What happens between the dashboard handing over a terminal and getting it back: show the
//! surface, show the surfaces side by side if asked, remember the arrangement, and say how it
//! went. The binary only loops (dashboard, [`visit`], dashboard).

mod returning;
mod terminals;
mod visiting;

#[cfg(test)]
mod visit_tests;

pub use returning::Returning;
pub use terminals::Terminals;
pub use visiting::visit;
