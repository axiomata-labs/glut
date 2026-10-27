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
    /// The tokens the session's own calls wrote while their answers streamed, and how long that took (TUI10): the
    /// speed the model writes at.
    pub written: u64,
    pub writing: std::time::Duration,
    /// How long the turn's first call took to its first piece of answer.
    pub first_token: Option<std::time::Duration>,
}

impl Turn {
    /// Tokens a second the model wrote at this turn; `None` where nothing streamed.
    pub fn rate(&self) -> Option<f64> {
        let seconds = self.writing.as_secs_f64();
        (self.written > 0 && seconds > 0.05).then(|| self.written as f64 / seconds)
    }
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
    /// The prices of each subagent on a model of its own (T37a), by its `task` call: its `Started` says them.
    sub_prices: HashMap<String, Prices>,
    /// When the session's own call under way was sent — the turn's start, or the last tool's answer before it.
    call_sent: Option<std::time::Instant>,
    /// When the call under way sent its first piece, and how many characters it has sent since (TUI10).
    streaming: Option<(std::time::Instant, usize)>,
}

/// Characters a token of English or code holds, roughly: what the speed is guessed by while an answer streams, before
/// the provider counts its tokens.
const CHARS_PER_TOKEN: f64 = 4.0;

impl Meter {
    /// Tokens a second the answer streaming now comes at, guessed from its characters; `None` when none streams, or
    /// too briefly to say.
    pub fn live_rate(&self) -> Option<f64> {
        let (since, chars) = self.streaming?;
        let seconds = since.elapsed().as_secs_f64();
        (seconds > 0.3 && chars > 0).then(|| chars as f64 / CHARS_PER_TOKEN / seconds)
    }

    /// The speed of the last turn that streamed.
    pub fn last_rate(&self) -> Option<f64> {
        self.turns.iter().rev().find_map(Turn::rate)
    }
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
    /// What the meter says where a subscription's use stands instead of money (PV4).
    pub plan: Option<String>,
}

impl Meter {
    /// Takes in one event of the session's runs, its cost at `prices`.
    pub fn take(&mut self, event: &Event, prices: Option<Prices>) {
        self.take_at(event, prices, 0);
    }

    fn take_at(&mut self, event: &Event, prices: Option<Prices>, depth: u32) {
        // The next call goes once the tools have answered: its wait is counted from the last of them (TUI10).
        if depth == 0 && matches!(event, Event::ToolResult { .. }) {
            self.call_sent = Some(std::time::Instant::now());
        }
        match event {
            Event::Started { .. } if depth == 0 => {
                let number = self.turns.len() + 1;
                self.turns.push(Turn {
                    number,
                    ..Turn::default()
                });
                self.call_sent = Some(std::time::Instant::now());
                self.streaming = None;
            }
            // The session's own answer streaming (TUI10): when it began, and how much came.
            Event::TextDelta { text, .. } | Event::ReasoningDelta { text, .. } if depth == 0 => {
                let now = std::time::Instant::now();
                match &mut self.streaming {
                    Some((_, chars)) => *chars += text.chars().count(),
                    None => {
                        self.streaming = Some((now, text.chars().count()));
                        let waited = self.call_sent.map(|sent| now.duration_since(sent));
                        let turn = self.turn();
                        if turn.first_token.is_none() {
                            turn.first_token = waited;
                        }
                    }
                }
            }
            Event::Sub { depth, task, event } => {
                // Each start says it anew: a provider that numbers its calls the same in every reply reuses a task id.
                if let (Some(task), Event::Started { prices, .. }) = (task, &**event) {
                    match prices {
                        Some(own) => self.sub_prices.insert(task.clone(), *own),
                        None => self.sub_prices.remove(task),
                    };
                }
                let prices = task
                    .as_ref()
                    .and_then(|task| self.sub_prices.get(task).copied())
                    .or(prices);
                self.take_at(event, prices, *depth);
            }
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
                // The session's own call ends: what it wrote, and how long its answer streamed.
                let streamed = if depth == 0 {
                    self.call_sent = Some(std::time::Instant::now());
                    self.streaming.take().map(|(since, _)| since.elapsed())
                } else {
                    None
                };
                let turn = self.turn();
                if let Some(took) = streamed {
                    turn.written += *output_tokens;
                    turn.writing += took;
                }
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
        let mut lines = vec![head(tr!(meter_context))];
        match window.window {
            Some((size, source)) => {
                lines.push(bar(window.used, u64::from(size), window.compacts_at, width));
                lines.push(Line::from(Span::raw(fit(format!(
                    "  {}",
                    tr!(
                        meter_of,
                        used = tokens(window.used),
                        size = tokens(u64::from(size)),
                        share = window.used * 100 / u64::from(size.max(1))
                    )
                )))));
                lines.push(quiet(match window.compacts_at {
                    Some(at) => format!("  {}", tr!(meter_compacts_at, at = tokens(at))),
                    None => format!("  {}", tr!(meter_compacts_manual)),
                }));
                // Where the window came from, as the session named it — its own words read in the language now.
                let source = match source {
                    crate::session::WINDOW_SET_HERE => tr!(window_set_here),
                    crate::session::WINDOW_FROM_LIST => tr!(window_from_list),
                    other => other,
                };
                lines.push(quiet(format!("  {}", tr!(meter_window, source = source))));
            }
            None => {
                lines.push(Line::from(Span::raw(fit(format!(
                    "  {}",
                    tr!(meter_used, used = tokens(window.used))
                )))));
                lines.push(quiet(format!("  {}", tr!(meter_window_unknown))));
            }
        }

        let (all, cost) = (window.usage, window.cost);
        lines.push(Line::default());
        lines.push(head(tr!(meter_session)));
        lines.push(Line::from(Span::raw(fit(format!(
            "  {}",
            tr!(
                meter_in_out,
                input = tokens(all.input_tokens),
                cached = cached_share(&all),
                output = tokens(all.output_tokens)
            )
        )))));
        lines.push(quiet(match cost {
            Some(cost) => format!("  {}", tr!(meter_price, cost = glut::lang::usd(cost, 4))),
            None if window.plan.is_some() => {
                format!("  {}", window.plan.clone().unwrap_or_default())
            }
            None => format!("  {}", tr!(meter_no_price)),
        }));
        if let Some(effort) = &window.effort {
            lines.push(quiet(format!("  {}", tr!(meter_effort, effort = effort))));
        }
        if let Some(budget) = &window.budget {
            lines.push(quiet(format!("  {}", tr!(meter_budget, budget = budget))));
        }

        lines.push(Line::default());
        lines.push(head("Glut lean"));
        let (calls, before, after) = self.lean;
        lines.push(quiet(if calls == 0 {
            format!("  {}", tr!(meter_lean_none))
        } else {
            format!(
                "  {}",
                tr!(
                    meter_lean_session,
                    chars = count(before.saturating_sub(after)),
                    calls = calls,
                    share = before.saturating_sub(after) * 100 / before.max(1)
                )
            )
        }));
        if let Some((calls, saved)) = self.lean_all {
            lines.push(quiet(format!(
                "  {}",
                tr!(meter_lean_all, chars = count(saved), calls = calls)
            )));
        }

        lines.push(Line::default());
        lines.push(head(tr!(meter_turns)));
        if self.turns.is_empty() {
            lines.push(quiet(format!("  {}", tr!(meter_none_yet))));
        }
        for turn in self.turns.iter().rev().take(TURNS_LISTED) {
            let cost = turn
                .cost
                .map(|cost| format!(" · {}", glut::lang::usd(cost, 4)))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("  #{:<3}", turn.number), theme::glow()),
                Span::raw(crate::transcript::cut(
                    &tr!(
                        meter_turn_in_out,
                        input = tokens(turn.usage.input_tokens),
                        output = tokens(turn.usage.output_tokens),
                        cost = cost
                    ),
                    width.saturating_sub(6),
                )),
            ]));
            lines.push(quiet(format!(
                "       {}",
                tr!(
                    meter_turn_detail,
                    cached = cached_share(&turn.usage),
                    calls = turn.calls,
                    tools = turn.tools
                )
            )));
            // TUI10: how fast the model wrote, and how long the first piece took.
            let speed = match (turn.rate(), turn.first_token) {
                (Some(rate), Some(first)) => Some(format!(
                    "       {rate:.0} tok/s · {}",
                    tr!(
                        meter_first_token,
                        secs = glut::lang::decimal(first.as_secs_f64(), 1)
                    )
                )),
                (Some(rate), None) => Some(format!("       {rate:.0} tok/s")),
                (None, Some(first)) => Some(format!(
                    "       {}",
                    tr!(
                        meter_first_token,
                        secs = glut::lang::decimal(first.as_secs_f64(), 1)
                    )
                )),
                (None, None) => None,
            };
            if let Some(speed) = speed {
                lines.push(quiet(speed));
            }
        }

        lines.push(Line::default());
        lines.push(head(tr!(meter_tools)));
        if self.tools.is_empty() {
            lines.push(quiet(format!("  {}", tr!(meter_none_called))));
        }
        let mut tools: Vec<(&String, &ToolCount)> = self.tools.iter().collect();
        tools.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then(a.0.cmp(b.0)));
        for (name, tool) in tools.iter().take(TOOLS_LISTED) {
            let failed = if tool.failed > 0 {
                format!(" · {}", tr!(meter_failed, n = tool.failed))
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
            lines.push(quiet(format!(
                "  {}",
                tr!(more_listed, n = tools.len() - TOOLS_LISTED)
            )));
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
        1_000..1_000_000 => format!("{}k", glut::lang::decimal(count as f64 / 1_000.0, 1)),
        _ => format!("{}M", glut::lang::decimal(count as f64 / 1_000_000.0, 1)),
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
            prices: None,
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
    fn the_speed_counts_the_streamed_answers_tokens_over_their_time_and_the_wait_for_the_first() {
        let mut meter = Meter::default();
        meter.take(
            &Event::Started {
                model: "m".into(),
                cwd: String::new(),
                tools: Vec::new(),
                prices: None,
            },
            None,
        );
        meter.take(
            &Event::TextDelta {
                turn: 1,
                text: "Hello world, this streams".into(),
            },
            None,
        );
        std::thread::sleep(std::time::Duration::from_millis(350));
        assert!(meter.live_rate().is_some(), "guessed while it streams");
        meter.take(
            &Event::Usage {
                turn: 1,
                input_tokens: 10,
                cached_input_tokens: 0,
                output_tokens: 70,
            },
            None,
        );
        assert!(meter.live_rate().is_none());
        let turn = &meter.turns[0];
        assert_eq!(turn.written, 70);
        assert!(turn.first_token.is_some());
        let rate = meter.last_rate().unwrap();
        assert!(rate > 50.0 && rate < 250.0, "{rate}");
    }

    #[test]
    fn a_subagent_on_a_model_of_its_own_is_priced_by_its_own() {
        let mine = Prices {
            input_per_m: 1.0,
            output_per_m: 1.0,
            cached_input_per_m: None,
        };
        let own = Prices {
            input_per_m: 10.0,
            ..mine
        };
        let sub = |event: Event| Event::Sub {
            depth: 1,
            task: Some("c1".into()),
            event: Box::new(event),
        };
        let mut meter = Meter::default();
        meter.take(&started(), Some(mine));
        meter.take(
            &sub(Event::Started {
                model: "big".into(),
                cwd: ".".into(),
                tools: Vec::new(),
                prices: Some(own),
            }),
            Some(mine),
        );
        meter.take(&sub(usage(1_000_000, 0, 0)), Some(mine));
        meter.take(&usage(1_000_000, 0, 0), Some(mine));
        let cost = meter.turns[0].cost.unwrap();
        assert!((cost - 11.0).abs() < 1e-9, "{cost}");
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
            window: Some((200_000, crate::session::WINDOW_FROM_LIST)),
            compacts_at: Some(160_000),
            effort: Some("high".into()),
            budget: None,
            usage: Usage::default(),
            cost: Some(12_345.678),
            plan: None,
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
