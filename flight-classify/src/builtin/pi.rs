// SPDX-License-Identifier: MIT

//! Pi manifest. Pi is wired by an extension rather than scraped; the only screen rule is a
//! spinner-only composer row, anchored so braille in transcript text does not match.

use crate::{AgentKind, ClassifyError, Manifest, Rule};
use flight_state::AgentState::Busy;

pub(super) fn manifest() -> Result<Manifest, ClassifyError> {
    let screen = vec![Rule::new(
        "busy.spinner-glyph",
        10,
        r"(?m)^[│┃][ \t]*[\x{2801}-\x{28FF}][ \t]*[│┃][ \t]*$",
        Busy,
    )?];
    Manifest::new(AgentKind::Pi, 15, None, screen, Vec::new())
}
