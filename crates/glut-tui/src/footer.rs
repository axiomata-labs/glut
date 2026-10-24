//! The footer (`docs/plans/glut-tui.md`, T39): the session's status as coloured segments — the vi state, the mode,
//! the model, the context, the cost, the branch — with the key hints beside them, as much of both as the line holds.
//! Powerline's arrows join the segments where the terminal draws them; plain blocks where it may not.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

use crate::theme;

/// Powerline's arrow after a segment (U+E0B0, a private-use glyph only patched fonts and some terminals draw).
pub const ARROW: &str = "\u{e0b0}";
/// The arrow pointing the other way (U+E0B2), where the hints' bar starts.
const ARROW_LEFT: &str = "\u{e0b2}";
/// The rounded ends (U+E0B6 facing left, U+E0B4 facing right): a bar is round on the outside, pointed toward the
/// middle of the line (the owner's look, after Powerlevel10k).
pub const ROUND_LEFT: &str = "\u{e0b6}";
const ROUND_RIGHT: &str = "\u{e0b4}";
/// Powerline's branch glyph (U+E0A0); the plain footer has Unicode's own branch sign.
const BRANCH_POWERLINE: &str = "\u{e0a0}";
const BRANCH_PLAIN: &str = "⎇";
/// The share of where the run compacts from which the context glows, and from which it is the fault colour.
const CONTEXT_HEED: f64 = 0.8;
const CONTEXT_FAULT: f64 = 0.95;

/// How the segments are joined (`/footer`, kept in `prefs.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Separators {
    Powerline,
    Plain,
}

impl Separators {
    pub fn label(self) -> &'static str {
        match self {
            Separators::Powerline => "powerline",
            Separators::Plain => "plain",
        }
    }

    pub fn branch_sign(self) -> &'static str {
        match self {
            Separators::Powerline => BRANCH_POWERLINE,
            Separators::Plain => BRANCH_PLAIN,
        }
    }
}

/// A kept choice as `prefs.json` holds it, leniently: a value this Glut does not know (a hand's typo, a later Glut's
/// word) is no choice, rather than a file whose every other pref is lost.
///
/// # Errors
///
/// Never for a value; only what the reader itself fails at.
pub fn lenient<'de, D: serde::Deserializer<'de>>(
    reader: D,
) -> Result<Option<Separators>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(reader)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

/// The separators for a terminal that names itself so, and the terminal they were chosen for: no program can ask a
/// font for its glyphs, but Ghostty, WezTerm and kitty draw Powerline's themselves whatever the font.
pub fn guess(term_program: Option<&str>, term: Option<&str>) -> (Separators, Option<&'static str>) {
    let program = term_program.unwrap_or_default().to_lowercase();
    let term = term.unwrap_or_default().to_lowercase();
    let found = if program == "ghostty" || term.contains("ghostty") {
        Some("Ghostty")
    } else if program == "wezterm" || term.contains("wezterm") {
        Some("WezTerm")
    } else if term.contains("kitty") {
        Some("kitty")
    } else {
        None
    };
    match found {
        Some(name) => (Separators::Powerline, Some(name)),
        None => (Separators::Plain, None),
    }
}

/// One segment: its text, its own colours or `None` for the calm ones (they take turns between two backgrounds), and
/// when it goes on a short line — `0` never, else the highest first.
#[derive(Debug, Clone)]
pub struct Segment {
    pub text: String,
    pub style: Option<Style>,
    pub drop: u8,
}

impl Segment {
    pub fn calm(text: impl Into<String>, drop: u8) -> Self {
        Segment {
            text: text.into(),
            style: None,
            drop,
        }
    }

    /// A segment on `bg` in the theme's ink, bold: what wants to be seen.
    pub fn marked(text: impl Into<String>, bg: Color, drop: u8) -> Self {
        Segment {
            text: text.into(),
            style: Some(
                Style::default()
                    .bg(bg)
                    .fg(theme::c().ink)
                    .add_modifier(Modifier::BOLD),
            ),
            drop,
        }
    }
}

/// The context's segment: `ctx 41k/200k`, glowing from [`CONTEXT_HEED`] of where the run compacts (of the window
/// where it does not), the fault colour from [`CONTEXT_FAULT`].
pub fn context(used: u64, window: Option<u32>, compacts_at: Option<u64>) -> Segment {
    let tokens = crate::meter::tokens;
    let text = match window {
        Some(size) => format!("ctx {}/{}", tokens(used), tokens(u64::from(size))),
        None => format!("ctx {}", tokens(used)),
    };
    let limit = compacts_at
        .or(window.map(u64::from))
        .filter(|limit| *limit > 0);
    let share = limit.map_or(0.0, |limit| used as f64 / limit as f64);
    if share >= CONTEXT_FAULT {
        Segment::marked(text, theme::c().fault, 0)
    } else if share >= CONTEXT_HEED {
        Segment::marked(text, theme::c().glow, 0)
    } else {
        Segment::calm(text, 0)
    }
}

/// The two calm backgrounds, mixed from the theme's ink (its background) and its quiet grey: quiet in a dark theme
/// and a light one alike, the terminal's own text colour readable on both.
pub fn calm_backgrounds() -> [Color; 2] {
    let palette = theme::c();
    [
        theme::mix(palette.ink, palette.quiet, 0.25),
        theme::mix(palette.ink, palette.quiet, 0.4),
    ]
}

/// The width `segments` take when joined by `separators`.
fn segments_width(segments: &[&Segment], separators: Separators) -> usize {
    let texts: usize = segments
        .iter()
        .map(|segment| segment.text.width() + 2)
        .sum();
    let joints = match separators {
        // The round start, and an arrow after each, the last one included: it closes the bar.
        Separators::Powerline if segments.is_empty() => 0,
        Separators::Powerline => segments.len() + 1,
        Separators::Plain => segments.len().saturating_sub(1),
    };
    texts + joints
}

/// The footer `width` cells wide: the segments on the left, the hints (`a · b · c`) quiet on the right. A short line
/// loses the hints item by item first, then the segments by their `drop` — unless the hints are the moment's (`moment`:
/// an ask, a list, a dialog, a toast), which then go only after every segment that may. The segments that never go
/// (`drop` 0) are cut at the edge on a line too short even for them.
pub fn line(
    segments: Vec<Segment>,
    hints: &str,
    width: usize,
    separators: Separators,
    moment: bool,
) -> Line<'static> {
    let mut kept: Vec<&Segment> = segments.iter().collect();
    // The hints' bar has its arrow, its round end and a space on either side in Powerline's look.
    let hints_frame = match separators {
        Separators::Powerline => 4,
        Separators::Plain => 0,
    };
    let hints_room = if moment {
        hints.width() + hints_frame + 1
    } else {
        0
    };
    while segments_width(&kept, separators) + hints_room > width {
        let Some(most) = kept
            .iter()
            .map(|segment| segment.drop)
            .max()
            .filter(|most| *most > 0)
        else {
            break;
        };
        let at = kept
            .iter()
            .rposition(|segment| segment.drop == most)
            .unwrap_or(0);
        kept.remove(at);
    }
    let backgrounds = calm_backgrounds();
    let mut calm = 0;
    let styles: Vec<Style> = kept
        .iter()
        .map(|segment| {
            segment.style.unwrap_or_else(|| {
                let bg = backgrounds[calm % 2];
                calm += 1;
                Style::default().bg(bg).fg(Color::Reset)
            })
        })
        .collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    if let (Separators::Powerline, Some(first)) = (separators, styles.first()) {
        spans.push(Span::styled(
            ROUND_LEFT,
            Style::default().fg(first.bg.unwrap_or(Color::Reset)),
        ));
    }
    for (at, segment) in kept.iter().enumerate() {
        spans.push(Span::styled(format!(" {} ", segment.text), styles[at]));
        match separators {
            Separators::Powerline => {
                let next = styles
                    .get(at + 1)
                    .and_then(|style| style.bg)
                    .unwrap_or(Color::Reset);
                let here = styles[at].bg.unwrap_or(Color::Reset);
                spans.push(Span::styled(ARROW, Style::default().fg(here).bg(next)));
            }
            Separators::Plain if at + 1 < kept.len() => spans.push(Span::raw(" ")),
            Separators::Plain => {}
        }
    }
    let used = segments_width(&kept, separators);
    let room = width.saturating_sub(used + 1 + hints_frame);
    let shown = fitted_hints(hints, room);
    if !shown.is_empty() {
        let gap = width.saturating_sub(used + shown.width() + hints_frame);
        spans.push(Span::raw(" ".repeat(gap)));
        match separators {
            // The mirror of the segments' bar: pointed toward the middle, round at the edge.
            Separators::Powerline => {
                let bg = backgrounds[0];
                spans.push(Span::styled(ARROW_LEFT, Style::default().fg(bg)));
                spans.push(Span::styled(
                    format!(" {shown} "),
                    Style::default().bg(bg).fg(Color::Reset),
                ));
                spans.push(Span::styled(ROUND_RIGHT, Style::default().fg(bg)));
            }
            Separators::Plain => spans.push(Span::styled(shown, theme::quiet())),
        }
    }
    Line::from(spans)
}

/// As many of the hints, from the first, as `room` holds whole; the first one cut where not even it fits.
fn fitted_hints(hints: &str, room: usize) -> String {
    let mut shown = String::new();
    for item in hints.split(" · ") {
        let next = if shown.is_empty() {
            item.to_owned()
        } else {
            format!("{shown} · {item}")
        };
        if next.width() > room {
            break;
        }
        shown = next;
    }
    if shown.is_empty() && room > 1 {
        return crate::transcript::cut(hints, room);
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn status() -> Vec<Segment> {
        vec![
            Segment::calm("build", 0),
            Segment::calm("openrouter · glm-4.7", 1),
            Segment::calm("ctx 41.0k/200.0k", 0),
            Segment::calm("$0.0123", 2),
            Segment::calm("⎇ main", 3),
        ]
    }

    #[test]
    fn an_unknown_footer_in_the_prefs_is_no_choice_and_keeps_the_rest() {
        let prefs: crate::prefs::Prefs =
            serde_json::from_str(r#"{"footer": "fancy", "vi": true}"#).unwrap();
        assert_eq!((prefs.footer, prefs.vi), (None, Some(true)));
        let prefs: crate::prefs::Prefs = serde_json::from_str(r#"{"footer": "plain"}"#).unwrap();
        assert_eq!(prefs.footer, Some(Separators::Plain));
    }

    #[test]
    fn the_terminals_that_draw_powerline_themselves_get_it_and_every_other_plain_blocks() {
        assert_eq!(
            guess(Some("ghostty"), None),
            (Separators::Powerline, Some("Ghostty"))
        );
        assert_eq!(guess(Some("WezTerm"), None).0, Separators::Powerline);
        assert_eq!(guess(None, Some("xterm-kitty")).0, Separators::Powerline);
        assert_eq!(
            guess(Some("Apple_Terminal"), Some("xterm-256color")),
            (Separators::Plain, None)
        );
        assert_eq!(guess(None, None).0, Separators::Plain);
    }

    #[test]
    fn a_short_line_loses_the_hints_first_then_the_branch_the_cost_and_the_model() {
        let hints = "⏎ send · ⇧⏎ new line · ⌃K keys";
        let wide = text(&line(status(), hints, 120, Separators::Plain, false));
        assert!(
            wide.contains("⎇ main") && wide.ends_with("⌃K keys"),
            "{wide:?}"
        );
        let fewer = text(&line(status(), hints, 82, Separators::Plain, false));
        assert!(
            fewer.contains("⎇ main") && fewer.ends_with("⏎ send") && !fewer.contains("⌃K"),
            "{fewer:?}"
        );
        let short = text(&line(status(), hints, 52, Separators::Plain, false));
        assert!(
            short.contains("build") && short.contains("ctx") && short.contains("glm"),
            "{short:?}"
        );
        assert!(!short.contains("main") && !short.contains('$'), "{short:?}");
        let least = text(&line(status(), hints, 10, Separators::Plain, false));
        assert!(
            least.contains("build") && least.contains("ctx"),
            "mode and context always stay"
        );
        for width in [120, 82, 52, 40] {
            assert!(text(&line(status(), hints, width, Separators::Plain, false)).width() <= width);
        }
    }

    #[test]
    fn the_moments_hints_go_before_the_segments_that_may() {
        let hints = "y once · 1–3 for this session · n refuse";
        let shown = text(&line(status(), hints, 76, Separators::Powerline, true));
        assert!(
            shown.ends_with(&format!("n refuse {ROUND_RIGHT}")),
            "{shown:?}"
        );
        assert!(!shown.contains("main"), "{shown:?}");
        assert!(shown.width() <= 76, "{shown:?}");
    }

    #[test]
    fn powerline_bars_are_round_outside_and_pointed_toward_the_middle_and_plain_puts_a_space_between()
     {
        let segments = vec![Segment::calm("a", 0), Segment::calm("b", 0)];
        assert_eq!(
            text(&line(
                segments.clone(),
                "",
                40,
                Separators::Powerline,
                false
            )),
            format!("{ROUND_LEFT} a {ARROW} b {ARROW}")
        );
        assert_eq!(
            text(&line(
                vec![Segment::calm("a", 0)],
                "keys",
                20,
                Separators::Powerline,
                false
            )),
            format!(
                "{ROUND_LEFT} a {ARROW}{}{ARROW_LEFT} keys {ROUND_RIGHT}",
                " ".repeat(6)
            )
        );
        assert_eq!(
            text(&line(segments, "", 40, Separators::Plain, false)),
            " a   b "
        );
    }

    #[test]
    fn the_context_glows_near_where_the_run_compacts_and_turns_to_the_fault_colour_at_it() {
        assert!(context(100, Some(200_000), Some(160_000)).style.is_none());
        // Compared with each other, not with the palette: another test may switch the theme meanwhile.
        let near = context(130_000, Some(200_000), Some(160_000))
            .style
            .unwrap();
        let at = context(155_000, Some(200_000), Some(160_000))
            .style
            .unwrap();
        assert!(near.bg.is_some() && at.bg.is_some() && near.bg != at.bg);
        let over = context(190_000, Some(200_000), None).style.unwrap();
        assert!(
            over.bg.is_some(),
            "the window counts where the run does not compact"
        );
        assert!(
            context(190_000, None, None).style.is_none(),
            "no window, nothing to measure against"
        );
    }
}
