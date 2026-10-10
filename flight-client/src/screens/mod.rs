// SPDX-License-Identifier: MIT

//! What each showing surface has drawn, and putting it on one terminal (ADR-011).
//!
//! A surface's attachment delivers the bytes a tmux client writes to a terminal. To show two
//! of them at once, each is kept in a [`ScreenModel`] (a grid of cells with attributes, a
//! cursor and the terminal modes the program asked for), and [`paint`] draws the models into
//! the regions a `flight_present::Solved` layout gives them. Flight does not parse escape
//! sequences itself: the terminal emulator is behind the `TerminalEngine` trait (`engine.rs`),
//! implemented once, in the one file that names the emulator library (`emulator.rs`).

mod cell;
mod emulator;
mod engine;
mod geometry;
mod modes;
mod paint;
mod screen_model;

pub use cell::{CellView, Colour};
pub use geometry::{Geometry, TooSmall};
pub use modes::{Modes, MouseMode};
pub use paint::{paint, Painted, Theme};
pub use screen_model::ScreenModel;

#[cfg(test)]
mod engine_contract_tests;
#[cfg(test)]
mod screen_tests;
