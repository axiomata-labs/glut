//! The gate, live (T15, "Wache"): every judgement of the session's gate as it comes — allowed by which rule or
//! mode, asked and how the owner answered, refused and why —, the hosts the network proxy was asked about, and the
//! refusals of the turn against the limits that end it (#112). The side panel's Gate tab shows it; `/gate` lists the
//! rules in force and takes an "always" back.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use glut::Event;
use glut::config::RefusalLimits;
use glut::sandbox::proxy::NETWORK_TOOL;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr as _;

use crate::theme;

/// How many judgements the watch keeps: the newest, a long session's first ones are in its scrollback.
const KEPT: usize = 200;
/// How many "always" answers and hosts the tab lists before it says how many more there are.
const LISTED: usize = 8;

/// How one call fared at the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A rule or the mode let it through.
    Allowed,
    /// The owner was asked and allowed it.
    AskedAllowed,
    /// The owner was asked and refused it.
    AskedRefused,
    /// A rule, the mode or the missing owner refused it.
    Refused,
}

impl Verdict {
    /// The gate says "when asked" in the reason of every judgement the owner answered (`permissions::Gate::judge`).
    fn of(allowed: bool, reason: &str) -> Self {
        let asked = reason.contains("when asked");
        match (allowed, asked) {
            (true, false) => Verdict::Allowed,
            (true, true) => Verdict::AskedAllowed,
            (false, true) => Verdict::AskedRefused,
            (false, false) => Verdict::Refused,
        }
    }

    fn mark(self) -> (&'static str, Style) {
        match self {
            Verdict::Allowed => ("✓", theme::quiet()),
            Verdict::AskedAllowed => ("?✓", theme::glow()),
            Verdict::AskedRefused => ("?✗", theme::fault()),
            Verdict::Refused => ("✗", theme::fault()),
        }
    }
}

/// One judgement as the watch keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub tool: String,
    pub target: String,
    pub verdict: Verdict,
    pub reason: String,
    /// 0 for the session's own run, deeper for a subagent's.
    pub depth: u32,
}

/// How often the proxy let a host through and refused it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Host {
    pub reached: u32,
    pub refused: u32,
}

/// What the gate did this session.
#[derive(Debug, Default)]
pub struct Watch {
    /// The newest judgements, oldest first.
    pub seen: VecDeque<Seen>,
    /// The proxy's hosts, by name.
    pub hosts: BTreeMap<String, Host>,
    /// The refusals of the turn under way (or the last one), a subagent's included — what the run's ledger counts.
    pub refused: u32,
    /// Per tool and target, how often this turn refused it: the ledger ends the run when one reaches its limit.
    same: HashMap<(String, String), u32>,
    /// The refusals counted this turn, by call, tool and target: the ledger counts one per call however often the
    /// proxy refused a host during it.
    counted: HashSet<(u32, String, String, String)>,
}

/// What the Gate tab says beside the judgements: how the session is guarded now.
pub struct Guard<'a> {
    pub mode: &'a str,
    /// The sandbox holds commands (on in the settings and there on the system); `None`: off in the settings.
    pub sandbox: Option<bool>,
    pub limits: RefusalLimits,
    /// The "always" answers of the session, as the asks named them.
    pub granted: &'a [String],
    /// The settings' rules: allow, ask, deny.
    pub rules: (usize, usize, usize),
}

impl Watch {
    /// Takes in one event of the session's runs.
    pub fn take(&mut self, event: &Event) {
        self.take_at(event, 0);
    }

    fn take_at(&mut self, event: &Event, depth: u32) {
        match event {
            // A new turn's run counts its refusals from none, as its ledger does.
            Event::Started { .. } if depth == 0 => {
                self.refused = 0;
                self.same.clear();
                self.counted.clear();
            }
            Event::Sub { depth, event, .. } => self.take_at(event, *depth),
            Event::Permission {
                id,
                tool,
                target,
                allowed,
                reason,
                ..
            } => {
                let verdict = Verdict::of(*allowed, reason);
                if tool == NETWORK_TOOL {
                    let host = self.hosts.entry(target.clone()).or_default();
                    if *allowed {
                        host.reached += 1;
                    } else {
                        host.refused += 1;
                    }
                }
                // As the ledger counts (`glut::refusals`): once per call, and a subagent's refusals only toward the
                // turn's total, never as the parent's repeated call.
                if !allowed
                    && self
                        .counted
                        .insert((depth, id.clone(), tool.clone(), target.clone()))
                {
                    self.refused += 1;
                    if depth == 0 {
                        *self.same.entry((tool.clone(), target.clone())).or_default() += 1;
                    }
                }
                if self.seen.len() == KEPT {
                    self.seen.pop_front();
                }
                self.seen.push_back(Seen {
                    tool: tool.clone(),
                    target: target.clone(),
                    verdict,
                    reason: reason.clone(),
                    depth,
                });
            }
            _ => {}
        }
    }

    /// The most often one call was refused this turn.
    pub fn most_repeated(&self) -> u32 {
        self.same.values().copied().max().unwrap_or(0)
    }

    /// The Gate tab's lines, `width` columns wide, as many as `height` holds: how the session is guarded, the turn's
    /// refusals, the "always" answers, the hosts, then the judgements newest first.
    pub fn lines(&self, guard: &Guard, width: usize, height: usize) -> Vec<Line<'static>> {
        // What a model or a command named reaches the screen cleaned of control characters, and cut to its room.
        let fit = |text: String, room: usize| {
            crate::transcript::cut(&crate::transcript::clean(&text).replace('\n', " ⏎ "), room)
        };
        let more = |left: usize| {
            Line::from(Span::styled(
                fit(format!("  {}", tr!(more_listed, n = left)), width),
                theme::quiet(),
            ))
        };
        let mut lines = Vec::new();
        let sandbox = match guard.sandbox {
            Some(true) => (tr!(gate_sandbox_on), theme::quiet()),
            Some(false) => (tr!(gate_sandbox_none), theme::fault()),
            None => (tr!(gate_sandbox_off), theme::fault()),
        };
        let mode = fit(format!(" {} · ", tr!(gate_mode, mode = guard.mode)), width);
        let left = width.saturating_sub(mode.width());
        lines.push(Line::from(vec![
            Span::styled(mode, theme::quiet()),
            Span::styled(fit(sandbox.0.to_owned(), left), sandbox.1),
        ]));
        let (allow, ask, deny) = guard.rules;
        lines.push(Line::from(Span::styled(
            fit(
                format!(
                    " {}",
                    tr!(gate_rules, allow = allow, ask = ask, deny = deny)
                ),
                width,
            ),
            theme::quiet(),
        )));
        // Near a limit the count glows: the next refusals end the turn.
        let limits = guard.limits;
        let close = self.refused + 3 >= limits.total || self.most_repeated() + 1 >= limits.repeat;
        let style = if self.refused == 0 {
            theme::quiet()
        } else if close {
            theme::fault()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(
            fit(
                format!(
                    " {}",
                    tr!(
                        gate_refused,
                        refused = self.refused,
                        total = limits.total,
                        same = self.most_repeated(),
                        repeat = limits.repeat
                    )
                ),
                width,
            ),
            style,
        )));
        if !guard.granted.is_empty() {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                fit(format!(" {}", tr!(gate_from_now)), width),
                theme::glow(),
            )));
            for label in guard.granted.iter().take(LISTED) {
                lines.push(Line::from(Span::raw(fit(format!("  ✓ {label}"), width))));
            }
            if guard.granted.len() > LISTED {
                lines.push(more(guard.granted.len() - LISTED));
            }
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            fit(format!(" {}", tr!(gate_network)), width),
            theme::glow(),
        )));
        if self.hosts.is_empty() {
            lines.push(Line::from(Span::styled(
                fit(format!("  {}", tr!(gate_no_hosts)), width),
                theme::quiet(),
            )));
        }
        for (name, host) in self.hosts.iter().take(LISTED) {
            let (mark, style) = if host.refused > 0 && host.reached == 0 {
                ("✗", theme::fault())
            } else {
                ("✓", Style::default())
            };
            let mut count = format!("{}×", host.reached);
            if host.refused > 0 {
                count = tr!(
                    gate_host_refused,
                    reached = host.reached,
                    refused = host.refused
                );
            }
            lines.push(Line::from(vec![
                Span::styled(format!("  {mark} "), style),
                Span::styled(fit(name.clone(), width.saturating_sub(4)), style),
            ]));
            lines.push(Line::from(Span::styled(
                fit(format!("    {count}"), width),
                theme::quiet(),
            )));
        }
        if self.hosts.len() > LISTED {
            lines.push(more(self.hosts.len() - LISTED));
        }
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            fit(format!(" {}", tr!(gate_judgements)), width),
            theme::glow(),
        )));
        if self.seen.is_empty() {
            lines.push(Line::from(Span::styled(
                fit(format!("  {}", tr!(gate_no_judgements)), width),
                theme::quiet(),
            )));
        }
        let room = height.saturating_sub(lines.len());
        for seen in self.seen.iter().rev().take(room / 2) {
            let (mark, style) = seen.verdict.mark();
            let nested = if seen.depth > 0 { "↳ " } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("  {mark:<2} "), style),
                Span::raw(fit(
                    format!("{nested}{} {}", seen.tool, seen.target),
                    width.saturating_sub(5),
                )),
            ]));
            lines.push(Line::from(Span::styled(
                fit(format!("     {}", shown_reason(&seen.reason)), width),
                theme::quiet(),
            )));
        }
        // The judgements fill what the rest leaves; on a short panel the rest is cut too, never pushed off.
        lines.truncate(height);
        lines
    }
}

/// A judgement's reason as the screen words it: the gate names a mode as the settings write it (`mode accept_edits`),
/// the footer as `build`.
fn shown_reason(reason: &str) -> String {
    match reason.strip_prefix("mode ") {
        Some(name) if glut::permissions::Mode::parse(name).is_some() => {
            format!("mode {}", crate::session::mode_label_of(name))
        }
        _ => reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_mode_in_a_judgement_is_named_as_the_footer_names_it() {
        assert_eq!(shown_reason("mode accept_edits"), "mode build");
        assert_eq!(shown_reason("mode default"), "mode ask");
        assert_eq!(shown_reason("allowed when asked"), "allowed when asked");
        assert_eq!(shown_reason("mode of its own"), "mode of its own");
    }
    use super::*;

    fn judged(tool: &str, target: &str, allowed: bool, reason: &str) -> Event {
        Event::Permission {
            turn: 1,
            id: "c".into(),
            tool: tool.into(),
            target: target.into(),
            allowed,
            reason: reason.into(),
        }
    }

    #[test]
    fn judgements_hosts_and_the_turns_refusals_are_counted() {
        let mut watch = Watch::default();
        watch.take(&judged("bash", "ls", true, "rule Bash(ls)"));
        watch.take(&judged("bash", "rm x", false, "refused when asked (mode)"));
        watch.take(&judged(
            NETWORK_TOOL,
            "crates.io",
            true,
            "rule Network(crates.io)",
        ));
        watch.take(&judged(NETWORK_TOOL, "evil.org", false, "mode build"));
        watch.take(&Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(judged(NETWORK_TOOL, "evil.org", false, "mode build")),
        });
        // The proxy refused the host twice in one call: the ledger counts it once.
        watch.take(&judged(NETWORK_TOOL, "evil.org", false, "mode build"));
        watch.seen.pop_back();
        assert_eq!(watch.seen.len(), 5);
        assert_eq!(watch.seen[1].verdict, Verdict::AskedRefused);
        assert_eq!(
            watch.seen[4].depth, 1,
            "a subagent's judgement is kept as its"
        );
        assert_eq!(watch.refused, 3, "a subagent's refusals count for the turn");
        assert_eq!(
            watch.most_repeated(),
            1,
            "but never as the parent's repeated call"
        );
        // The same refused in a call of its own repeats it.
        let mut again = judged(NETWORK_TOOL, "evil.org", false, "mode build");
        if let Event::Permission { id, .. } = &mut again {
            *id = "d".into();
        }
        watch.take(&again);
        watch.seen.pop_back();
        assert_eq!((watch.refused, watch.most_repeated()), (4, 2));
        assert_eq!(
            watch.hosts["evil.org"],
            Host {
                reached: 0,
                refused: 4
            },
            "the hosts count every connection"
        );
        // A subagent's start is not a new turn; the session's own is.
        watch.take(&Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(Event::Started {
                model: "m".into(),
                cwd: ".".into(),
                tools: Vec::new(),
                prices: None,
            }),
        });
        assert_eq!(watch.refused, 4);
        watch.take(&Event::Started {
            model: "m".into(),
            cwd: ".".into(),
            tools: Vec::new(),
            prices: None,
        });
        assert_eq!((watch.refused, watch.most_repeated()), (0, 0));
        assert_eq!(watch.seen.len(), 5, "the judgements stay for the session");
    }

    #[test]
    fn the_watch_keeps_only_the_newest_and_its_lines_fit_the_width() {
        let mut watch = Watch::default();
        for at in 0..KEPT + 5 {
            watch.take(&judged(
                "bash",
                &format!("echo {at} {}", "x".repeat(80)),
                true,
                "mode auto",
            ));
        }
        assert_eq!(watch.seen.len(), KEPT);
        assert!(watch.seen[0].target.starts_with("echo 5 "));
        let granted = ["git status *".to_owned()];
        let guard = Guard {
            mode: "build",
            sandbox: Some(true),
            limits: RefusalLimits::DEFAULT,
            granted: &granted,
            rules: (2, 0, 1),
        };
        let lines = watch.lines(&guard, 30, 20);
        assert!(lines.len() <= 20, "{}", lines.len());
        for line in &lines {
            assert!(line.width() <= 30, "{line:?}");
        }
        let text: Vec<String> = lines.iter().map(crate::select::text_of).collect();
        assert!(
            text.iter().any(|line| line.contains("git status *")),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|line| line.contains(&format!("echo {}", KEPT + 4))),
            "the newest first"
        );
    }
}
