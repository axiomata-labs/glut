//! Glut's colours (T5, "Glut + Hex"): orange only where something lives, works or needs the owner — the running
//! step, an open question, the input's cursor — quiet greys for what is settled. The terminal's own background and
//! text colour stay, so Glut sits in any theme.

use ratatui::style::{Color, Modifier, Style};

/// Glut's orange (`--ax-harness-glut` in Axiomata's tokens).
pub const GLOW: Color = Color::Rgb(0xe8, 0xb0, 0x4a);
/// The glow's darker edge, for the mascot's outline and a waiting mark.
pub const EMBER: Color = Color::Rgb(0xa8, 0x5a, 0x1e);
/// Settled things: a finished tool line, a hint.
pub const QUIET: Color = Color::Rgb(0x8a, 0x8f, 0x98);
/// The owner's own words.
pub const OWNER: Color = Color::Rgb(0x9c, 0xc8, 0xe8);
/// Something failed.
pub const FAULT: Color = Color::Rgb(0xe0, 0x6c, 0x6c);
/// Something went right.
pub const FINE: Color = Color::Rgb(0x8c, 0xc8, 0x8c);
/// Dark text on the glow (the search's match shown).
pub const INK: Color = Color::Rgb(0x1a, 0x1a, 0x1a);
/// A search's other matches: a dim amber, quieter than the glow.
pub const MATCH: Color = Color::Rgb(0x5a, 0x44, 0x20);

pub fn quiet() -> Style {
    Style::default().fg(QUIET)
}

pub fn glow() -> Style {
    Style::default().fg(GLOW)
}

pub fn strong() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
