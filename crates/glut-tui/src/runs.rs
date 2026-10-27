//! The Runs tab (`docs/plans/glut-tui.md`, T60b): every plan of every project as a small tile — when it runs next,
//! how its last run ended, with its cost and the answer's first line. Opened, a plan shows its task, what it may do
//! and its last ten runs; Enter opens a run's session, `r` runs the plan now, `p` pauses or resumes it, `d` deletes
//! it once the owner says yes. A line at the top points to the loops running in the Agents tab (T60a).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use crossterm::event::KeyCode;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::schedule::{Outcome, Plan, Record, Schedules};
use crate::theme;
use crate::transcript::{cut, wrap};

/// How many runs an opened plan shows, newest first.
const SHOWN_RUNS: usize = 10;
/// How long what the tab read stays: the file and the records are read again after it while the tab is drawn.
const FRESH: Duration = Duration::from_secs(2);

/// A plan as the tab shows it: the plan, and its last runs, newest first.
struct Shown {
    plan: Plan,
    runs: Vec<Record>,
}

/// The tab's plans, and where its keys stand.
#[derive(Default)]
pub struct Board {
    /// The tab's keys are its list's.
    pub keys: bool,
    /// The chosen plan, by its place in the list.
    pub chosen: usize,
    /// The chosen plan opened, and the run chosen among its last ones.
    pub opened: Option<usize>,
    /// `d` was pressed: the next key says yes (`y`) or no.
    pub deleting: bool,
    /// `r` was pressed: the next key says yes (`y`) or no — a run spends and acts with nobody watching.
    pub running: bool,
    plans: Vec<Shown>,
    /// Why the plans cannot be read.
    pub fault: Option<String>,
    read: Option<Instant>,
}

/// What a key on the tab asks the app for.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// A run's session, in its plan's folder.
    Open { session: String, folder: PathBuf },
    /// The plan run now.
    Run(String),
    /// The plan paused (`true`) or running again.
    Hold(String, bool),
    /// The plan deleted, the owner said yes.
    Delete(String),
}

impl Board {
    /// The plans and their runs read again, unless they were a moment ago.
    pub fn refresh(&mut self, home: &Path) {
        if self.read.is_some_and(|read| read.elapsed() < FRESH) {
            return;
        }
        self.read = Some(Instant::now());
        match Schedules::load(home) {
            Ok(schedules) => {
                self.fault = None;
                self.plans = schedules
                    .plans
                    .into_iter()
                    .map(|plan| {
                        let mut runs = crate::schedule::runs(home, &plan.name);
                        runs.reverse();
                        runs.truncate(SHOWN_RUNS);
                        Shown { plan, runs }
                    })
                    .collect();
            }
            Err(reason) => {
                self.fault = Some(reason);
                self.plans.clear();
            }
        }
        self.chosen = self.chosen.min(self.plans.len().saturating_sub(1));
        if self.plans.is_empty() {
            self.opened = None;
            self.deleting = false;
            self.running = false;
        }
    }

    /// Read again at the next look: a key changed the plans.
    pub fn stale(&mut self) {
        self.read = None;
    }

    /// How many plans there are.
    pub fn len(&self) -> usize {
        self.plans.len()
    }

    /// The plan `name` chosen, its tile in front (a tile's click).
    pub fn choose(&mut self, name: &str) {
        if let Some(at) = self.plans.iter().position(|shown| shown.plan.name == name) {
            self.chosen = at;
            self.opened = None;
            self.deleting = false;
            self.running = false;
        }
    }

    /// The chosen plan's name.
    pub fn chosen_name(&self) -> Option<&str> {
        self.plans
            .get(self.chosen)
            .map(|shown| shown.plan.name.as_str())
    }

    /// Esc or ⌫: the question, then the opened plan, closed first — `false` when nothing was open.
    pub fn back(&mut self) -> bool {
        if self.deleting || self.running {
            self.deleting = false;
            self.running = false;
            return true;
        }
        self.opened.take().is_some()
    }

    /// A key on the tab: `None` when the tab does not use it, else what the app is to do, if anything.
    pub fn key(&mut self, code: KeyCode) -> Option<Option<Act>> {
        let shown = self.plans.get(self.chosen)?;
        let name = shown.plan.name.clone();
        if self.deleting {
            self.deleting = false;
            return Some((code == KeyCode::Char('y')).then_some(Act::Delete(name)));
        }
        if self.running {
            self.running = false;
            return Some((code == KeyCode::Char('y')).then_some(Act::Run(name)));
        }
        let step = |at: usize, len: usize, down: bool| {
            if down {
                (at + 1).min(len.saturating_sub(1))
            } else {
                at.saturating_sub(1)
            }
        };
        match code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                let down = matches!(code, KeyCode::Down | KeyCode::Char('j'));
                match &mut self.opened {
                    Some(run) => *run = step(*run, shown.runs.len(), down),
                    None => self.chosen = step(self.chosen, self.plans.len(), down),
                }
            }
            KeyCode::Enter => match self.opened {
                None => self.opened = Some(0),
                Some(run) => {
                    let session = shown.runs.get(run).and_then(|run| run.session.clone());
                    return Some(session.map(|session| Act::Open {
                        session,
                        folder: shown.plan.folder.clone(),
                    }));
                }
            },
            KeyCode::Backspace => {
                self.back();
            }
            KeyCode::Char('r') => self.running = true,
            KeyCode::Char('p') => return Some(Some(Act::Hold(name, !shown.plan.paused))),
            KeyCode::Char('d') => self.deleting = true,
            _ => return None,
        }
        Some(None)
    }

    /// The tab's rows at `width` in at most `room` rows, each with the plan a click on it chooses; `loops` loops run in
    /// the panes now.
    pub fn lines(
        &self,
        home: &Path,
        width: usize,
        room: usize,
        loops: usize,
        now: DateTime<Utc>,
    ) -> Vec<(Option<usize>, Line<'static>)> {
        let mut lines = Vec::new();
        let quiet = |text: String| {
            (
                None,
                Line::from(Span::styled(format!(" {text}"), theme::quiet())),
            )
        };
        if loops > 0 {
            for row in wrap(&tr!(runs_loops, n = loops), width) {
                lines.push(quiet(row));
            }
            lines.push((None, Line::default()));
        }
        if let Some(fault) = &self.fault {
            for row in wrap(fault, width) {
                lines.push((
                    None,
                    Line::from(Span::styled(format!(" {row}"), theme::fault())),
                ));
            }
            return lines;
        }
        if self.plans.is_empty() {
            for row in wrap(tr!(runs_none), width) {
                lines.push(quiet(row));
            }
            return lines;
        }
        if let Some(run) = self.opened {
            let opened = self.opened_lines(home, width, run);
            // The chosen run stays in view: the details go up past the top as far as it needs.
            let room_left = room.saturating_sub(lines.len()).max(1);
            let chosen = opened
                .iter()
                .position(|(_, line)| line.spans.iter().any(|span| span.content.starts_with(" ▸")))
                .unwrap_or(0);
            let skip = (chosen + 2).saturating_sub(room_left);
            lines.extend(opened.into_iter().skip(skip));
        } else {
            // The chosen tile stays in view: the list starts as late as it must.
            let tiles: Vec<Vec<Line<'static>>> = self
                .plans
                .iter()
                .enumerate()
                .map(|(at, shown)| tile(home, shown, width, at == self.chosen, now))
                .collect();
            let mut first = 0;
            while first < self.chosen
                && tiles[first..=self.chosen]
                    .iter()
                    .map(Vec::len)
                    .sum::<usize>()
                    > room.saturating_sub(lines.len())
            {
                first += 1;
            }
            for (at, rows) in tiles.into_iter().enumerate().skip(first) {
                lines.extend(rows.into_iter().map(|row| (Some(at), row)));
            }
        }
        lines.truncate(room.max(1));
        lines
    }

    /// The chosen plan opened: what it does and may do, and its last runs, `run` chosen.
    fn opened_lines(
        &self,
        home: &Path,
        width: usize,
        run: usize,
    ) -> Vec<(Option<usize>, Line<'static>)> {
        let Some(shown) = self.plans.get(self.chosen) else {
            return Vec::new();
        };
        let plan = &shown.plan;
        let mut lines = vec![(
            Some(self.chosen),
            Line::from(Span::styled(
                format!(" ◂ {}", cut(&plan.name, width.saturating_sub(2))),
                theme::strong(),
            )),
        )];
        let mut said = |text: &str, style: Style| {
            for row in wrap(text, width.saturating_sub(2)) {
                lines.push((None, Line::from(Span::styled(format!("   {row}"), style))));
            }
        };
        said(
            &format!(
                "{} · {}",
                plan.when_shown(),
                crate::projects::shown_path(&plan.folder, home)
            ),
            theme::quiet(),
        );
        said(
            &tr!(
                schedule_shown_task,
                task = crate::transcript::clean(&plan.task)
            ),
            Style::default(),
        );
        said(
            &tr!(
                schedule_shown_mode,
                mode = crate::session::mode_label(plan.mode),
                means = crate::scheduled::means(plan.mode)
            ),
            theme::quiet(),
        );
        if !plan.allow.is_empty() {
            said(
                &tr!(schedule_shown_allow, rules = plan.allow.join(", ")),
                theme::quiet(),
            );
        }
        if !plan.deny.is_empty() {
            said(
                &tr!(schedule_shown_deny, rules = plan.deny.join(", ")),
                theme::quiet(),
            );
        }
        if !plan.folders.is_empty() {
            said(
                &tr!(schedule_shown_folders, folders = plan.folders.join(", ")),
                theme::quiet(),
            );
        }
        said(
            &tr!(
                runs_limits,
                usd = glut::lang::usd(plan.max_usd, 2),
                minutes = plan.max_minutes,
                model = plan.model,
                provider = plan.provider
            ),
            theme::quiet(),
        );
        lines.push((None, Line::default()));
        if shown.runs.is_empty() {
            lines.push((
                None,
                Line::from(Span::styled(
                    format!("   {}", tr!(schedule_never)),
                    theme::quiet(),
                )),
            ));
            return lines;
        }
        lines.push((
            None,
            Line::from(Span::styled(
                format!("   {}", tr!(runs_last_ten)),
                theme::strong(),
            )),
        ));
        for (at, record) in shown.runs.iter().enumerate() {
            let picked = at == run;
            let mark = if picked { "▸" } else { " " };
            let when = record
                .started
                .with_timezone(&chrono::Local)
                .format(glut::lang::date_time())
                .to_string();
            let head = format!(
                "{} {} · {}",
                outcome_mark(record.outcome),
                when,
                crate::scheduled::cost_shown(record.cost)
            );
            let style = if picked {
                theme::glow()
            } else {
                Style::default()
            };
            lines.push((
                None,
                Line::from(Span::styled(
                    format!(" {mark} {}", cut(&head, width.saturating_sub(2))),
                    style,
                )),
            ));
            if !record.said.is_empty() {
                lines.push((
                    None,
                    Line::from(Span::styled(
                        format!("     {}", cut(&record.said, width.saturating_sub(4))),
                        theme::quiet(),
                    )),
                ));
            }
        }
        lines
    }
}

/// How a run ended, as one mark: ✓ answered, ✗ else, – skipped.
fn outcome_mark(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Answered => "✓",
        Outcome::Skipped => "–",
        _ => "✗",
    }
}

/// A plan's tile: its name, when and where it runs, when next, how its last run ended and what it said.
fn tile(
    home: &Path,
    shown: &Shown,
    width: usize,
    picked: bool,
    now: DateTime<Utc>,
) -> Vec<Line<'static>> {
    let plan = &shown.plan;
    let last = shown.runs.first();
    let (mark, head_style) = if picked {
        ("▸", theme::glow())
    } else {
        (" ", theme::strong())
    };
    let mut rows = vec![Line::from(Span::styled(
        format!(" {mark} {}", cut(&plan.name, width.saturating_sub(2))),
        head_style,
    ))];
    let mut quiet = |text: &str| {
        rows.push(Line::from(Span::styled(
            format!("   {}", cut(text, width.saturating_sub(2))),
            theme::quiet(),
        )));
    };
    quiet(&format!(
        "{} · {}",
        plan.when_shown(),
        crate::projects::shown_path(&plan.folder, home)
    ));
    let next = match plan.next(crate::schedule::last_timed(&shown.runs), now) {
        Some(next) => tr!(
            runs_next,
            at = next
                .with_timezone(&chrono::Local)
                .format(glut::lang::date_time())
        ),
        None => tr!(schedule_paused_mark).to_owned(),
    };
    quiet(&next);
    match last {
        Some(run) => {
            quiet(&format!(
                "{} {}",
                outcome_mark(run.outcome),
                tr!(
                    schedule_last,
                    outcome = run.outcome.said(),
                    ago = crate::projects::ago(run.ended, now),
                    cost = crate::scheduled::cost_shown(run.cost)
                )
            ));
            if !run.said.is_empty() {
                quiet(&run.said);
            }
        }
        None => quiet(tr!(schedule_never)),
    }
    rows.push(Line::default());
    rows
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;
    use crate::schedule::When;

    fn plan(name: &str) -> Plan {
        Plan {
            name: name.to_owned(),
            folder: PathBuf::from("/projects/app"),
            task: "run the tests".to_owned(),
            when: When::Every { minutes: 60 },
            provider: "p".to_owned(),
            model: "m".to_owned(),
            mode: glut::Mode::Default,
            allow: Vec::new(),
            deny: Vec::new(),
            folders: Vec::new(),
            max_usd: 1.0,
            max_minutes: 30,
            paused: false,
            path: String::new(),
            changed: Utc.with_ymd_and_hms(2026, 10, 27, 6, 0, 0).unwrap(),
        }
    }

    fn record(n: i64, outcome: Outcome, session: Option<&str>) -> Record {
        Record {
            started: Utc.timestamp_opt(1_800_000_000 + n * 3600, 0).unwrap(),
            ended: Utc.timestamp_opt(1_800_000_000 + n * 3600 + 60, 0).unwrap(),
            by_hand: false,
            outcome,
            cost: Some(0.02),
            said: format!("said {n}"),
            session: session.map(str::to_owned),
            refused: Vec::new(),
            unpriced: false,
        }
    }

    fn board() -> Board {
        Board {
            plans: vec![
                Shown {
                    plan: plan("tests"),
                    runs: vec![
                        record(2, Outcome::Answered, Some("s-2")),
                        record(1, Outcome::Failed, None),
                    ],
                },
                Shown {
                    plan: Plan {
                        paused: true,
                        ..plan("sync")
                    },
                    runs: Vec::new(),
                },
            ],
            read: Some(Instant::now()),
            ..Board::default()
        }
    }

    fn text(lines: &[(Option<usize>, Line<'static>)]) -> String {
        lines
            .iter()
            .map(|(_, line)| {
                line.spans
                    .iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_keys_choose_open_a_runs_session_and_ask_before_a_delete() {
        let mut board = board();
        assert_eq!(board.key(KeyCode::Down), Some(None));
        assert_eq!(board.chosen_name(), Some("sync"));
        assert_eq!(
            board.key(KeyCode::Down),
            Some(None),
            "the last stays chosen"
        );
        assert_eq!(
            board.key(KeyCode::Char('p')),
            Some(Some(Act::Hold("sync".into(), false))),
            "paused: resumed"
        );
        board.key(KeyCode::Up);
        assert_eq!(
            board.key(KeyCode::Char('p')),
            Some(Some(Act::Hold("tests".into(), true)))
        );
        // r asks too: a run spends and acts with nobody watching.
        assert_eq!(board.key(KeyCode::Char('r')), Some(None));
        assert!(board.running);
        assert_eq!(
            board.key(KeyCode::Char('y')),
            Some(Some(Act::Run("tests".into())))
        );
        // Opened: Enter on the newest run opens its session; one without a kept session opens nothing.
        assert_eq!(board.key(KeyCode::Enter), Some(None));
        assert_eq!(board.opened, Some(0));
        assert_eq!(
            board.key(KeyCode::Enter),
            Some(Some(Act::Open {
                session: "s-2".into(),
                folder: "/projects/app".into()
            }))
        );
        board.key(KeyCode::Char('j'));
        assert_eq!(board.key(KeyCode::Enter), Some(None));
        assert!(board.back() && board.opened.is_none());
        assert!(!board.back(), "nothing open: Esc is the panel's");
        // d asks; any key but y says no.
        assert_eq!(board.key(KeyCode::Char('d')), Some(None));
        assert!(board.deleting);
        assert_eq!(board.key(KeyCode::Char('n')), Some(None));
        assert!(!board.deleting);
        board.key(KeyCode::Char('d'));
        assert_eq!(
            board.key(KeyCode::Char('y')),
            Some(Some(Act::Delete("tests".into())))
        );
        assert_eq!(
            board.key(KeyCode::Char('z')),
            None,
            "a key the tab does not use is the input's"
        );
        assert_eq!(
            Board::default().key(KeyCode::Enter),
            None,
            "no plans: nothing to take"
        );
    }

    #[test]
    fn a_tile_shows_when_a_plan_runs_next_and_how_it_ran_last_and_opened_its_runs() {
        let home = PathBuf::from("/Users/o");
        let mut board = board();
        let now = Utc.timestamp_opt(1_800_000_000 + 3 * 3600, 0).unwrap();
        let shown = text(&board.lines(&home, 60, 40, 2, now));
        assert!(shown.starts_with(" ↻ 2"), "{shown}");
        assert!(shown.contains("▸ tests"));
        assert!(
            shown.contains("said 2") && !shown.contains("said 1"),
            "the last run's line only: {shown}"
        );
        assert!(shown.contains(tr!(schedule_paused_mark)));
        assert!(shown.contains(tr!(schedule_never)));
        board.key(KeyCode::Enter);
        let opened = text(&board.lines(&home, 60, 40, 0, now));
        assert!(
            opened.contains("◂ tests") && opened.contains("run the tests"),
            "{opened}"
        );
        assert!(
            opened.contains("said 1") && opened.contains("✗"),
            "{opened}"
        );
        assert!(
            text(&board.lines(&home, 60, 3, 0, now)).lines().count() <= 3,
            "never more than the room"
        );
        let none = text(&Board::default().lines(&home, 60, 40, 0, now));
        assert!(none.contains(&wrap(tr!(runs_none), 60)[0]));
    }
}
