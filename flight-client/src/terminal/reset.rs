// SPDX-License-Identifier: MIT

/// Written to the user's terminal when a full-screen surface (or the switch from one to the
/// next) is over: leave the alternate screen, show the cursor, reset attributes, and switch off
/// the mouse reporting and bracketed paste the surface may have left on.
pub(crate) const FULL_SCREEN: &[u8] =
    b"\x1b[?1049l\x1b[?25h\x1b[0m\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l";

/// The same for a presentation, which owns the alternate screen and the mouse reporting: leave
/// it, show the cursor, reset attributes, mouse reporting, bracketed paste and application
/// cursor keys.
pub(crate) const PRESENTATION: &[u8] =
    b"\x1b[?1049l\x1b[?25h\x1b[0m\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[?1l";
