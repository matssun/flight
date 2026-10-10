// SPDX-License-Identifier: MIT

/// A colour as a program asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// One cell of a screen, read in place: it borrows the text from the screen it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellView<'a> {
    /// What is shown in it: a character with any combining marks; empty for a blank, and for
    /// the right half of a wide character.
    pub text: &'a str,
    pub fg: Colour,
    pub bg: Colour,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    /// A wide character, which also covers the cell to its right.
    pub wide: bool,
    /// The right half of a wide character.
    pub continuation: bool,
}
