//! The stream into the terminal's own scrollback (T3, TUI2c): on F3 or `/scrollback` Glut steps off its screen and
//! prints the conversation where the terminal keeps it — its own scrolling, search and selection work on it — and
//! at the end of a session the whole of it stays there, above the shell's next prompt (as Pi does).

use std::io::{self, Write};

use crossterm::style::{Attribute, Color as Term, ContentStyle, StyledContent};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

/// Writes `lines` to `out` with their colours, one terminal line each.
///
/// # Errors
///
/// What writing to `out` gives.
pub fn print(out: &mut impl Write, lines: &[Line]) -> io::Result<()> {
    for line in lines {
        for span in &line.spans {
            // Every block's text went through `transcript::clean` when it was laid out, so no control character is in
            // it: this print trusts that (the test below holds every kind of block to it).
            write!(
                out,
                "{}",
                StyledContent::new(content_style(span.style), span.content.as_ref())
            )?;
        }
        writeln!(out)?;
    }
    out.flush()
}

/// A ratatui style as the terminal's.
fn content_style(style: Style) -> ContentStyle {
    let mut content = ContentStyle::new();
    content.foreground_color = style.fg.and_then(color);
    content.background_color = style.bg.and_then(color);
    let attributes = [
        (Modifier::BOLD, Attribute::Bold),
        (Modifier::DIM, Attribute::Dim),
        (Modifier::ITALIC, Attribute::Italic),
        (Modifier::UNDERLINED, Attribute::Underlined),
        (Modifier::REVERSED, Attribute::Reverse),
        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
    ];
    for (modifier, attribute) in attributes {
        if style.add_modifier.contains(modifier) {
            content.attributes.set(attribute);
        }
    }
    content
}

fn color(color: Color) -> Option<Term> {
    Some(match color {
        Color::Reset => return None,
        Color::Black => Term::Black,
        Color::Red => Term::DarkRed,
        Color::Green => Term::DarkGreen,
        Color::Yellow => Term::DarkYellow,
        Color::Blue => Term::DarkBlue,
        Color::Magenta => Term::DarkMagenta,
        Color::Cyan => Term::DarkCyan,
        Color::Gray => Term::Grey,
        Color::DarkGray => Term::DarkGrey,
        Color::LightRed => Term::Red,
        Color::LightGreen => Term::Green,
        Color::LightYellow => Term::Yellow,
        Color::LightBlue => Term::Blue,
        Color::LightMagenta => Term::Magenta,
        Color::LightCyan => Term::Cyan,
        Color::White => Term::White,
        Color::Rgb(r, g, b) => Term::Rgb { r, g, b },
        Color::Indexed(at) => Term::AnsiValue(at),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    #[test]
    fn lines_come_out_with_their_colours_and_nothing_else() {
        let lines = [
            Line::from(vec![
                Span::styled("bold", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" plain"),
            ]),
            Line::from(Span::styled(
                "rgb",
                Style::default().fg(Color::Rgb(1, 2, 3)),
            )),
        ];
        let mut out = Vec::new();
        print(&mut out, &lines).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\u{1b}[1mbold"), "{text:?}");
        assert!(text.contains(" plain\n"), "{text:?}");
        assert!(text.contains("38;2;1;2;3"), "{text:?}");
        assert_eq!(text.matches('\n').count(), 2);
    }

    #[test]
    fn no_block_brings_a_control_character_of_its_own_into_the_print() {
        use crate::transcript::{Kind, Thinking, Tone, Transcript};
        let evil = "a\u{1b}[2J\u{9b}31m\u{1b}]0;x\u{7}\u{7f}\u{202e}b";
        let mut stream = Transcript::default();
        stream.push(Kind::Owner(evil.into()));
        stream.push(Kind::Answer {
            text: format!("{evil} &#27;[2J [l](http://x/&#27;)\n\n```{evil}\n{evil}\n```"),
            streaming: false,
        });
        stream.push(Kind::Tool {
            id: "c".into(),
            name: evil.into(),
            target: evil.into(),
            state: crate::transcript::ToolState::Done,
            gate: Some(evil.into()),
            output: evil.into(),
            unfolded: true,
            depth: 0,
        });
        stream.note(evil, Tone::Quiet);
        stream.push(Kind::Thinking {
            text: evil.into(),
            streaming: false,
            flipped: true,
        });
        let lines: Vec<Line> = stream
            .blocks
            .iter_mut()
            .flat_map(|block| block.lines(60, Thinking::Folded).to_vec())
            .collect();
        let mut out = Vec::new();
        print(&mut out, &lines).unwrap();
        let text = String::from_utf8(out).unwrap();
        // Every escape is one of the print's own colour sequences: ESC [ digits and semicolons m.
        let mut rest = text.as_str();
        while let Some(at) = rest.find('\u{1b}') {
            let after = &rest[at + 1..];
            let end = after.find('m').expect("a colour sequence ends");
            assert!(after.starts_with('['), "{after:?}");
            assert!(
                after[1..end]
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == ';'),
                "{after:?}"
            );
            rest = &after[end + 1..];
        }
        let others = text
            .chars()
            .filter(|c| c.is_control() && !matches!(c, '\n' | '\u{1b}'))
            .count();
        assert_eq!(others, 0, "{text:?}");
        assert!(!text.contains('\u{202e}'));
    }
}
