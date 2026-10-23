//! The meter, exact (T16): what the session spent and where — per turn of the owner's, per tool, and what Glut lean
//! spared — for the side panel's Meter tab; the header's line is drawn from the same sums.

use std::collections::HashMap;

use glut::Event;
use glut::config::Prices;
use glut::model::Usage;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::theme;

/// How many turns the tab lists, the newest first.
const TURNS_LISTED: usize = 8;
/// How many tools the tab lists, the most called first.
const TOOLS_LISTED: usize = 10;

/// One turn of the owner's: one run, its subagents included.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Turn {
    /// Counted from 1 in the session.
    pub number: usize,
    pub usage: Usage,
    pub cost: Option<f64>,
    /// The model's calls, a subagent's included.
    pub calls: u32,
    /// Tool calls.
    pub tools: u32,
}

/// How often one tool was called and failed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolCount {
    pub calls: u32,
    pub failed: u32,
}

/// What the session spent, by turn and by tool.
#[derive(Debug, Default)]
pub struct Meter {
    /// Every turn of the session, oldest first.
    pub turns: Vec<Turn>,
    pub tools: HashMap<String, ToolCount>,
    /// What Glut lean spared this session: calls and characters before and after.
    pub lean: (usize, usize, usize),
    /// What Glut lean spared on every run so far (`glut gain`): calls and characters spared, read when the tab opens.
    pub lean_all: Option<(usize, usize)>,
}

/// What the Meter tab says beside the sums: the window and how the turn under way stands.
pub struct Window {
    /// The prompt's tokens at the last model call.
    pub used: u64,
    /// The window, and where it comes from.
    pub window: Option<(u32, &'static str)>,
    /// From where the run compacts itself; `None` when it does not (switched off, no window).
    pub compacts_at: Option<u64>,
    pub effort: Option<String>,
    pub budget: Option<String>,
    /// The session's sums as the header shows them — a `/compact` and a handoff included, which are no turn.
    pub usage: Usage,
    pub cost: Option<f64>,
}

impl Meter {
    /// Takes in one event of the session's runs, its cost at `prices`.
    pub fn take(&mut self, event: &Event, prices: Option<Prices>) {
        self.take_at(event, prices, 0);
    }

    fn take_at(&mut self, event: &Event, prices: Option<Prices>, depth: u32) {
        match event {
            Event::Started { .. } if depth == 0 => {
                let number = self.turns.len() + 1;
                self.turns.push(Turn {
                    number,
                    ..Turn::default()
                });
            }
            Event::Sub { depth, event, .. } => self.take_at(event, prices, *depth),
            Event::Usage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                ..
            } => {
                let call = Usage {
                    input_tokens: *input_tokens,
                    cached_input_tokens: *cached_input_tokens,
                    output_tokens: *output_tokens,
                };
                let turn = self.turn();
                turn.usage += call;
                turn.calls += 1;
                if let Some(prices) = prices {
                    *turn.cost.get_or_insert(0.0) += prices.cost_usd(&call);
                }
            }
            Event::ToolCall { name, .. } => {
                self.turn().tools += 1;
                self.tools.entry(name.clone()).or_default().calls += 1;
            }
            Event::ToolResult {
                name, ok: false, ..
            } => {
                self.tools.entry(name.clone()).or_default().failed += 1;
            }
            Event::Lean {
                chars_before,
                chars_after,
                ..
            } => {
                self.lean.0 += 1;
                self.lean.1 += chars_before;
                self.lean.2 += chars_after;
            }
            _ => {}
        }
    }

    /// The turn under way; a usage before any start (a compaction of its own) opens one.
    fn turn(&mut self) -> &mut Turn {
        if self.turns.is_empty() {
            self.turns.push(Turn {
                number: 1,
                ..Turn::default()
            });
        }
        let last = self.turns.len() - 1;
        &mut self.turns[last]
    }

    /// Reads what Glut lean spared on every run so far, from its count at `path`.
    pub fn read_lean_all(&mut self, path: &std::path::Path) {
        self.lean_all = glut::lean::totals(path, None).ok().map(|totals| {
            (
                totals.iter().map(|total| total.calls).sum(),
                totals.iter().map(glut::lean::Total::saved).sum(),
            )
        });
    }

    /// The Meter tab's lines, `width` columns wide, as many as `height` holds.
    pub fn lines(&self, window: &Window, width: usize, height: usize) -> Vec<Line<'static>> {
        let fit = |text: String| crate::transcript::cut(&crate::transcript::clean(&text), width);
        let quiet = |text: String| Line::from(Span::styled(fit(text), theme::quiet()));
        let head = |text: &str| Line::from(Span::styled(fit(format!(" {text}")), theme::glow()));
        let mut lines = vec![head("Context")];
        match window.window {
            Some((size, source)) => {
                lines.push(bar(window.used, u64::from(size), window.compacts_at, width));
                lines.push(Line::from(Span::raw(fit(format!(
                    "  {} of {} ({}%)",
                    tokens(window.used),
                    tokens(u64::from(size)),
                    window.used * 100 / u64::from(size.max(1))
                )))));
                lines.push(quiet(match window.compacts_at {
                    Some(at) => format!("  compacts itself at {} (│)", tokens(at)),
                    None => "  compacts only on /compact".to_owned(),
                }));
                lines.push(quiet(format!("  window: {source} · /context sets it")));
            }
            None => {
                lines.push(Line::from(Span::raw(fit(format!(
                    "  {} used",
                    tokens(window.used)
                )))));
                lines.push(quiet("  window unknown: /context 128k sets it".to_owned()));
            }
        }

        let (all, cost) = (window.usage, window.cost);
        lines.push(Line::default());
        lines.push(head("Session"));
        lines.push(Line::from(Span::raw(fit(format!(
            "  in {} · cached {} · out {}",
            tokens(all.input_tokens),
            cached_share(&all),
            tokens(all.output_tokens)
        )))));
        lines.push(quiet(match cost {
            Some(cost) => format!("  ${cost:.4} at the model's price"),
            None => "  no price known for this model".to_owned(),
        }));
        if let Some(effort) = &window.effort {
            lines.push(quiet(format!("  effort {effort}")));
        }
        if let Some(budget) = &window.budget {
            lines.push(quiet(format!("  budget {budget}")));
        }

        lines.push(Line::default());
        lines.push(head("Glut lean"));
        let (calls, before, after) = self.lean;
        lines.push(quiet(if calls == 0 {
            "  this session: nothing spared yet".to_owned()
        } else {
            format!(
                "  this session: {} chars spared on {calls} calls ({}%)",
                count(before.saturating_sub(after)),
                before.saturating_sub(after) * 100 / before.max(1)
            )
        }));
        if let Some((calls, saved)) = self.lean_all {
            lines.push(quiet(format!(
                "  all runs: {} chars on {calls} calls (glut gain)",
                count(saved)
            )));
        }

        lines.push(Line::default());
        lines.push(head("Turns"));
        if self.turns.is_empty() {
            lines.push(quiet("  none yet".to_owned()));
        }
        for turn in self.turns.iter().rev().take(TURNS_LISTED) {
            let cost = turn
                .cost
                .map(|cost| format!(" · ${cost:.4}"))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("  #{:<3}", turn.number), theme::glow()),
                Span::raw(crate::transcript::cut(
                    &format!(
                        "in {} · out {}{cost}",
                        tokens(turn.usage.input_tokens),
                        tokens(turn.usage.output_tokens)
                    ),
                    width.saturating_sub(6),
                )),
            ]));
            lines.push(quiet(format!(
                "       {} cached · {} calls · {} tools",
                cached_share(&turn.usage),
                turn.calls,
                turn.tools
            )));
        }

        lines.push(Line::default());
        lines.push(head("Tools"));
        if self.tools.is_empty() {
            lines.push(quiet("  none called yet".to_owned()));
        }
        let mut tools: Vec<(&String, &ToolCount)> = self.tools.iter().collect();
        tools.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then(a.0.cmp(b.0)));
        for (name, tool) in tools.iter().take(TOOLS_LISTED) {
            let failed = if tool.failed > 0 {
                format!(" · {} failed", tool.failed)
            } else {
                String::new()
            };
            let style = if tool.failed > 0 {
                theme::fault()
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(
                fit(format!("  {:>4}× {name}{failed}", tool.calls)),
                style,
            )));
        }
        if tools.len() > TOOLS_LISTED {
            lines.push(quiet(format!("  … {} more", tools.len() - TOOLS_LISTED)));
        }
        lines.truncate(height);
        lines
    }
}

/// The context as a bar: used filled, the compaction point a `│`, the rest empty.
fn bar(used: u64, window: u64, compacts_at: Option<u64>, width: usize) -> Line<'static> {
    let cells = width.saturating_sub(4).clamp(4, 40);
    let window = window.max(1);
    let to_cell =
        |tokens: u64| usize::try_from(tokens.min(window) * cells as u64 / window).unwrap_or(cells);
    let filled = to_cell(used);
    let mark = compacts_at.map(to_cell);
    let near = compacts_at.is_some_and(|at| used * 10 >= at * 9);
    let style = if near { theme::fault() } else { theme::glow() };
    let mut spans = vec![Span::raw("  ")];
    for cell in 0..cells {
        let (text, style) = if Some(cell) == mark {
            ("│", theme::strong())
        } else if cell < filled {
            ("█", style)
        } else {
            ("░", theme::quiet())
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// The cached part of the input, with its share.
fn cached_share(usage: &Usage) -> String {
    if usage.cached_input_tokens == 0 {
        return "0".to_owned();
    }
    format!(
        "{} ({}%)",
        tokens(usage.cached_input_tokens),
        usage.cached_input_tokens * 100 / usage.input_tokens.max(1)
    )
}

/// A token count short: `950`, `12.3k`, `1.2M`.
pub fn tokens(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

/// A character count short.
fn count(chars: usize) -> String {
    tokens(chars as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> Event {
        Event::Started {
            model: "m".into(),
            cwd: ".".into(),
            tools: Vec::new(),
        }
    }

    fn usage(input: u64, cached: u64, output: u64) -> Event {
        Event::Usage {
            turn: 1,
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
        }
    }

    #[test]
    fn turns_tools_and_savings_are_summed_a_subagents_into_its_turn() {
        let prices = Prices {
            input_per_m: 1.0,
            output_per_m: 2.0,
            cached_input_per_m: None,
        };
        let mut meter = Meter::default();
        meter.take(&started(), Some(prices));
        meter.take(&usage(1_000_000, 500_000, 0), Some(prices));
        meter.take(&Event::tool_call(1, "c", "bash", "{}"), Some(prices));
        meter.take(
            &Event::tool_result(1, "c", "bash", false, "x"),
            Some(prices),
        );
        meter.take(
            &Event::Sub {
                depth: 1,
                task: None,
                event: Box::new(started()),
            },
            Some(prices),
        );
        meter.take(
            &Event::Sub {
                depth: 1,
                task: None,
                event: Box::new(usage(0, 0, 1_000_000)),
            },
            Some(prices),
        );
        meter.take(
            &Event::Lean {
                turn: 1,
                id: "c".into(),
                what: "cargo".into(),
                chars_before: 1000,
                chars_after: 100,
            },
            Some(prices),
        );
        meter.take(&started(), None);
        meter.take(&usage(10, 0, 5), None);
        assert_eq!(
            meter.turns.len(),
            2,
            "a subagent's start is no turn of the owner's"
        );
        let first = &meter.turns[0];
        assert_eq!((first.calls, first.tools), (2, 1));
        assert_eq!(first.usage.output_tokens, 1_000_000);
        assert!((first.cost.unwrap() - 3.0).abs() < 1e-9, "{:?}", first.cost);
        assert_eq!(meter.turns[1].cost, None, "no price, no cost");
        assert_eq!(
            meter.tools["bash"],
            ToolCount {
                calls: 1,
                failed: 1
            }
        );
        assert_eq!(meter.lean, (1, 1000, 100));
    }

    #[test]
    fn the_tab_fits_its_width_and_height_and_marks_the_compaction_point() {
        let mut meter = Meter::default();
        let prices = Prices {
            input_per_m: 1_000.0,
            output_per_m: 1_000.0,
            cached_input_per_m: None,
        };
        for _ in 0..20 {
            meter.take(&started(), Some(prices));
            meter.take(&usage(123_456_789, 0, 98_765_432), Some(prices));
            meter.take(
                &Event::tool_call(1, "c", "a_tool_with_a_very_long_name_indeed", "{}"),
                None,
            );
        }
        let window = Window {
            used: 150_000,
            window: Some((200_000, "the provider's list")),
            compacts_at: Some(160_000),
            effort: Some("high".into()),
            budget: None,
            usage: Usage::default(),
            cost: Some(12_345.678),
        };
        let lines = meter.lines(&window, 30, 25);
        assert!(lines.len() <= 25);
        for line in &lines {
            assert!(line.width() <= 30, "{line:?}");
        }
        let text: Vec<String> = lines.iter().map(crate::select::text_of).collect();
        assert!(text[1].contains('│'), "{:?}", text[1]);
        assert!(
            text.iter()
                .any(|line| line.contains("compacts itself at 160.0k"))
        );
        assert!(
            text.iter().any(|line| line.contains("#20")),
            "the newest turn first"
        );
    }
}
