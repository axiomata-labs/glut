//! The footer (T39): the session's status as coloured segments — the vi state, the mode,
//! the model, the context, the cost, the branch — with the key hints beside them, as much of both as the line holds.
//! Powerline's arrows join the segments where the terminal draws them; plain blocks where it may not.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

use crate::theme;

/// Powerline's arrow after a segment (U+E0B0, a private-use glyph only patched fonts and some terminals draw).
const ARROW: &str = "\u{e0b0}";
/// The arrow pointing the other way (U+E0B2), where the hints' bar starts.
const ARROW_LEFT: &str = "\u{e0b2}";
/// The rounded ends (U+E0B6 facing left, U+E0B4 facing right): a bar is round on the outside and where its segments
/// meet, pointed only at its inner end, toward the middle of the line (the owner's prompt, Powerlevel10k).
const ROUND_LEFT: &str = "\u{e0b6}";
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
    } else if program == "axiomata" {
        // Axiomata's panes draw with the official JetBrains Mono Nerd Font Mono, the same one Ghostty is set up with.
        Some("Axiomata")
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

    /// A segment on a muted `hue` (T84): its own colour, quiet beside what wants to be seen.
    pub fn hued(text: impl Into<String>, hue: Color, drop: u8) -> Self {
        if theme::plain() {
            return Segment::calm(text, drop);
        }
        Segment {
            text: text.into(),
            style: Some(Style::default().bg(theme::tint(hue)).fg(Color::Reset)),
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
        Segment::hued(text, theme::c().fine, 0)
    }
}

/// The two calm backgrounds, mixed from the theme's ink (its background) and its quiet grey: quiet in a dark theme
/// and a light one alike, the terminal's own text colour readable on both.
fn calm_backgrounds() -> [Color; 2] {
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
        // The round start, a joint after each segment and the point at the end: one cell each.
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
    let hints_room = if moment {
        hints_width(&hints.split(" · ").collect::<Vec<_>>(), separators) + 1
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
    let mut styles: Vec<Style> = kept
        .iter()
        .map(|segment| {
            segment.style.unwrap_or_else(|| {
                let bg = backgrounds[calm % 2];
                calm += 1;
                Style::default().bg(bg).fg(Color::Reset)
            })
        })
        .collect();
    // Two segments of one hue side by side would melt into one, their joint unseen (T84).
    for at in 1..styles.len() {
        if let (Some(bg), Some(before)) = (styles[at].bg, styles[at - 1].bg) {
            styles[at] = styles[at].bg(theme::apart_from(bg, before));
        }
    }
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
                // Round where one segment meets the next, pointed only at the bar's inner end.
                let joint = if at + 1 < kept.len() {
                    ROUND_RIGHT
                } else {
                    ARROW
                };
                spans.push(Span::styled(joint, Style::default().fg(here).bg(next)));
            }
            Separators::Plain if at + 1 < kept.len() => spans.push(Span::raw(" ")),
            Separators::Plain => {}
        }
    }
    let used = segments_width(&kept, separators);
    let room = width.saturating_sub(used + 1);
    let shown = fitted_hints(hints, room, separators);
    if !shown.is_empty() {
        let gap = width.saturating_sub(used + hints_width(&shown, separators));
        spans.push(Span::raw(" ".repeat(gap)));
        match separators {
            // The mirror of the segments' bar: each key its own segment, calm backgrounds by turns, pointed toward
            // the middle, round at the edge.
            Separators::Powerline => {
                let mut before = Color::Reset;
                for (at, item) in shown.iter().enumerate() {
                    let bg = backgrounds[at % 2];
                    let joint = if at == 0 { ARROW_LEFT } else { ROUND_LEFT };
                    spans.push(Span::styled(joint, Style::default().fg(bg).bg(before)));
                    spans.push(Span::styled(
                        format!(" {item} "),
                        Style::default().bg(bg).fg(Color::Reset),
                    ));
                    before = bg;
                }
                spans.push(Span::styled(ROUND_RIGHT, Style::default().fg(before)));
            }
            Separators::Plain => spans.push(Span::styled(shown.join(" · "), theme::quiet())),
        }
    }
    Line::from(spans)
}

/// The width the hint items take: in Powerline's look each its own segment (an arrow and a space either side) and
/// a round end; plain, joined by ` · `.
fn hints_width(items: &[impl AsRef<str>], separators: Separators) -> usize {
    if items.is_empty() {
        return 0;
    }
    let texts: usize = items.iter().map(|item| item.as_ref().width()).sum();
    match separators {
        Separators::Powerline => texts + items.len() * 3 + 1,
        Separators::Plain => texts + (items.len() - 1) * 3,
    }
}

/// As many of the hint items (`a · b · c`), from the first, as `room` holds whole; the first one cut where not even
/// it fits.
fn fitted_hints(hints: &str, room: usize, separators: Separators) -> Vec<String> {
    let items: Vec<&str> = hints.split(" · ").filter(|item| !item.is_empty()).collect();
    let mut shown: Vec<String> = Vec::new();
    for item in &items {
        shown.push((*item).to_owned());
        if hints_width(&shown, separators) > room {
            shown.pop();
            break;
        }
    }
    if shown.is_empty()
        && let Some(first) = items.first()
    {
        let frame = hints_width(&[""], separators);
        if room > frame + 1 {
            shown.push(crate::transcript::cut(first, room - frame));
        }
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
            guess(Some("Axiomata"), Some("xterm-256color")),
            (Separators::Powerline, Some("Axiomata"))
        );
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
    fn powerline_bars_are_round_outside_and_between_and_pointed_toward_the_middle_and_plain_puts_a_space_between()
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
            format!("{ROUND_LEFT} a {ROUND_RIGHT} b {ARROW}")
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
                " ".repeat(7)
            )
        );
        assert_eq!(
            text(&line(segments, "", 40, Separators::Plain, false)),
            " a   b "
        );
    }

    #[test]
    fn each_hint_is_a_segment_of_its_own_lighter_and_darker_by_turns() {
        let drawn = line(
            vec![Segment::calm("a", 0)],
            "⏎ send · ⌃K keys",
            40,
            Separators::Powerline,
            false,
        );
        assert_eq!(
            text(&drawn),
            format!(
                "{ROUND_LEFT} a {ARROW}{}{ARROW_LEFT} ⏎ send {ROUND_LEFT} ⌃K keys {ROUND_RIGHT}",
                " ".repeat(15)
            )
        );
        let backgrounds: Vec<_> = drawn
            .spans
            .iter()
            .filter(|span| span.content.contains("send") || span.content.contains("keys"))
            .map(|span| span.style.bg)
            .collect();
        assert_eq!(backgrounds.len(), 2);
        assert_ne!(backgrounds[0], backgrounds[1]);
        assert_eq!(text(&drawn).width(), 40);
    }

    #[test]
    fn the_context_glows_near_where_the_run_compacts_and_turns_to_the_fault_colour_at_it() {
        // The reader mode or `NO_COLOR` keeps the calm segments of before: nothing muted to compare.
        if theme::plain() {
            return;
        }
        // Far from it the context sits on its muted hue (T84), not in bold.
        let calm = context(100, Some(200_000), Some(160_000)).style.unwrap();
        assert!(!calm.add_modifier.contains(Modifier::BOLD));
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
            !context(190_000, None, None)
                .style
                .unwrap()
                .add_modifier
                .contains(Modifier::BOLD),
            "no window, nothing to measure against: the muted hue"
        );
    }
}
