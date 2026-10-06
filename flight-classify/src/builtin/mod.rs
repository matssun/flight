// SPDX-License-Identifier: MIT

//! Built-in manifests, ported from Fleet's `src/state/detection.ts`
//! (MIT, (c) 2026 Nick Nisi; see THIRD_PARTY.md). Rule ids, patterns and ordering keep
//! Fleet's meaning; ordering is expressed as explicit priorities (10, 20, ...).
//!
//! Pattern translation: JavaScript `\d` is written `[0-9]`, `/i` and `/m` flags are inline
//! `(?i)` / `(?m)`, and `⠀` is `\x{2800}`.

mod claude;
mod codex;
mod opencode;
mod pi;

use crate::{AgentKind, ClassifyError, Manifest};

/// Braille block U+2800-U+28FF: the animated progress glyph a harness paints only while it
/// is actively working.
pub(crate) const WORKING_GLYPH: &str = r"[\x{2800}-\x{28FF}]";

/// A leading braille frame plus a space in the pane title: the spinner a harness prepends
/// only while a turn is running.
pub(crate) const WORKING_TITLE: &str = r"^[\x{2800}-\x{28FF}] ";

pub(crate) fn manifest_for(agent: AgentKind) -> Result<Manifest, ClassifyError> {
    match agent {
        AgentKind::Claude => claude::manifest(),
        AgentKind::Codex => codex::manifest(),
        AgentKind::OpenCode => opencode::manifest(),
        AgentKind::Pi => pi::manifest(),
        AgentKind::Other => Manifest::new(AgentKind::Other, 15, None, Vec::new(), Vec::new()),
    }
}
