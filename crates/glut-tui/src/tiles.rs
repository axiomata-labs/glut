//! Tiles (T69): a small card at the window's top right when something the owner does not
//! see ends or asks — a background agent, another pane held as a tab, a subagent of a turn — its name, how it stands
//! and the first line of what it said. One goes after [`SHOWN_FOR`], one that asks stays until the question is
//! answered; at most [`MOST`] stand. They take no key: a click opens where it came from. `/notify tiles on|off`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::theme;
use crate::transcript::{clean, cut};

/// How long a tile stands that does not ask.
pub const SHOWN_FOR: Duration = Duration::from_secs(8);
/// Subagents of one pane that end within this of each other share a tile.
pub const FOLD_WITHIN: Duration = Duration::from_secs(2);
/// The most tiles that stand at once.
pub const MOST: usize = 3;
/// The widest a tile gets, in cells.
pub const WIDTH: u16 = 40;
/// The most lines inside one.
const MOST_LINES: usize = 3;

static ON: AtomicBool = AtomicBool::new(true);

/// The tests that read or set [`ON`] one at a time.
#[cfg(test)]
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds [`SERIAL`] for a test, whatever an earlier one left of it.
#[cfg(test)]
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether tiles are shown (`/notify tiles`, kept in `prefs.json`).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// How what a tile tells of stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stand {
    Done,
    Failed,
    Asks,
}

/// Where a click on a tile goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Goes {
    /// The background agent `name`: brought forward when it asks (its question is answered there), else the Agents
    /// tab at its card.
    Behind { name: String, asks: bool },
    /// The Agents tab of pane `pane` (its session's id), at the subagent of `path` — the newest of a shared tile.
    Subagent { pane: u64, path: Vec<usize> },
    /// Pane `pane` (its session's id), brought forward.
    Pane(u64),
    /// The Runs tab at the plan of this name (T60b): a run that ended while Glut was open.
    Runs(String),
}

/// One tile.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// Its head: an agent's name, a pane's, a subagent's task — or how many subagents ended.
    pub who: String,
    pub stand: Stand,
    pub lines: Vec<String>,
    pub goes: Goes,
    /// The session whose question it tells of: it stands while that one waits.
    pub asks_of: Option<u64>,
    /// How many subagents it tells of; 0 for anything else.
    pub subagents: usize,
    since: Instant,
    /// Where it was drawn last: a click finds it there.
    pub area: Rect,
}

impl Tile {
    pub fn new(who: String, stand: Stand, line: &str, goes: Goes) -> Self {
        let line = first_line(line);
        Tile {
            who,
            stand,
            lines: if line.is_empty() {
                Vec::new()
            } else {
                vec![line]
            },
            goes,
            asks_of: None,
            subagents: 0,
            since: Instant::now(),
            area: Rect::default(),
        }
    }
}

/// The first line of `text` with words in it, cleaned for the terminal.
fn first_line(text: &str) -> String {
    clean(
        text.lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim(),
    )
}

/// The tiles that stand, oldest first.
#[derive(Debug, Default)]
pub struct Tiles {
    pub list: Vec<Tile>,
}

impl Tiles {
    /// A tile more; a question of a session that asked before takes the place of its tile.
    pub fn push(&mut self, tile: Tile) {
        if !on() {
            return;
        }
        if let Some(asker) = tile.asks_of {
            self.list.retain(|old| old.asks_of != Some(asker));
        }
        self.list.push(tile);
        self.trim();
    }

    /// A subagent of pane `pane` ended — `ok`: as it was asked —, its task and what it answered: a tile of its own, or
    /// one more in the tile of those that ended just before it.
    pub fn subagent_ended(
        &mut self,
        pane: u64,
        path: Vec<usize>,
        task: &str,
        ok: bool,
        answer: &str,
    ) {
        if !on() {
            return;
        }
        let mark = if ok { "✓" } else { "✗" };
        let task = first_line(task);
        if let Some(tile) = self.list.iter_mut().rev().find(|tile| {
            matches!(&tile.goes, Goes::Subagent { pane: of, .. } if *of == pane)
                && tile.since.elapsed() < FOLD_WITHIN
        }) {
            // The first one's answer gives way to the tasks of all.
            if tile.subagents == 1 {
                let first = format!(
                    "{} {}",
                    if tile.stand == Stand::Done {
                        "✓"
                    } else {
                        "✗"
                    },
                    tile.who
                );
                tile.lines = vec![first];
            }
            tile.subagents += 1;
            tile.who = tr!(tiles_subagents, n = tile.subagents);
            if !ok {
                tile.stand = Stand::Failed;
            }
            tile.lines.push(format!("{mark} {task}"));
            tile.goes = Goes::Subagent { pane, path };
            tile.since = Instant::now();
            return;
        }
        let stand = if ok { Stand::Done } else { Stand::Failed };
        let mut tile = Tile::new(task, stand, answer, Goes::Subagent { pane, path });
        tile.subagents = 1;
        self.push(tile);
    }

    /// Once per frame: what stood its time goes, and a question once its session no longer `waits`.
    pub fn tick(&mut self, waits: impl Fn(u64) -> bool) {
        self.list.retain(|tile| match tile.asks_of {
            Some(asker) => waits(asker),
            None => tile.since.elapsed() < SHOWN_FOR,
        });
        if !on() {
            self.list.clear();
        }
    }

    /// At most [`MOST`]: the oldest that does not ask goes first — a question never, it waits for its answer.
    fn trim(&mut self) {
        while self.list.len() > MOST {
            let Some(at) = self.list.iter().position(|tile| tile.asks_of.is_none()) else {
                break;
            };
            self.list.remove(at);
        }
    }

    /// The tile under a click, taken away — where it goes.
    pub fn clicked(&mut self, column: u16, row: u16) -> Option<Goes> {
        let at = self.list.iter().position(|tile| {
            tile.area
                .contains(ratatui::layout::Position { x: column, y: row })
        })?;
        Some(self.list.remove(at).goes)
    }

    /// The tiles placed down from the top right of `area`, newest on top, each with its lines — none over the places
    /// in `keep_clear` (the questions' cards), none past half the screen's height.
    pub fn place(&mut self, area: Rect, keep_clear: &[Rect]) -> Vec<(Rect, Vec<Line<'static>>)> {
        for tile in &mut self.list {
            tile.area = Rect::default();
        }
        if area.width < 24 || area.height < 8 {
            return Vec::new();
        }
        let width = WIDTH.min(area.width - 2);
        let x = area.x + area.width - width - 1;
        let mut y = area.y + 1;
        let bottom = area.y + area.height / 2;
        let mut placed = Vec::new();
        for tile in self.list.iter_mut().rev() {
            let lines =
                tile.lines.len().min(MOST_LINES) + usize::from(tile.lines.len() > MOST_LINES);
            let height = lines as u16 + 2;
            if y + height > bottom {
                break;
            }
            let spot = Rect::new(x, y, width, height);
            y += height;
            if keep_clear.iter().any(|clear| clear.intersects(spot)) {
                continue;
            }
            tile.area = spot;
            placed.push((spot, rows(tile, usize::from(width))));
        }
        placed
    }
}

/// A tile's rows at `width`: its frame — the head and how it stands on it, the glow for a question, the fault's colour
/// for what broke off — and its lines inside.
fn rows(tile: &Tile, width: usize) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthStr;
    let (state, style) = match tile.stand {
        Stand::Done => (tr!(agent_done), theme::strong()),
        Stand::Failed => (tr!(agent_stopped), theme::fault()),
        Stand::Asks => (tr!(agent_asks_state), theme::glow()),
    };
    let inner = width.saturating_sub(4).max(1);
    let state = cut(state, inner / 2);
    // "╭─ ⬡ ", the head, " ─… ", the state, " ─╮".
    let who = cut(
        &clean(&tile.who).replace('\n', " "),
        width.saturating_sub(state.width() + 11).max(1),
    );
    let fill = width.saturating_sub(who.width() + state.width() + 10);
    let mut rows = vec![Line::from(vec![
        Span::styled("╭─ ⬡ ", style),
        Span::styled(who, theme::strong()),
        Span::styled(format!(" {} ", "─".repeat(fill)), style),
        Span::styled(state, style),
        Span::styled(" ─╮", style),
    ])];
    let mut inside: Vec<String> = tile.lines.iter().take(MOST_LINES).cloned().collect();
    if tile.lines.len() > MOST_LINES {
        inside.push(format!("… +{}", tile.lines.len() - MOST_LINES));
    }
    for line in inside {
        let line = cut(&line, inner);
        let pad = inner.saturating_sub(line.width());
        rows.push(Line::from(vec![
            Span::styled("│ ", style),
            Span::styled(line, theme::quiet()),
            Span::raw(" ".repeat(pad)),
            Span::styled(" │", style),
        ]));
    }
    let hint = cut(tr!(tile_opens), inner);
    let rule = width.saturating_sub(hint.width() + 6);
    rows.push(Line::from(vec![
        Span::styled(format!("╰{} ", "─".repeat(rule + 1)), style),
        Span::styled(hint, theme::quiet()),
        Span::styled(" ─╯", style),
    ]));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagents_that_end_together_share_a_tile_and_the_oldest_quiet_one_goes_first() {
        let _serial = serial();
        let mut tiles = Tiles::default();
        tiles.subagent_ended(1, vec![3], "review a.rs", true, "Two bugs.\nmore");
        assert_eq!(tiles.list.len(), 1);
        assert_eq!(tiles.list[0].lines, ["Two bugs."]);
        tiles.subagent_ended(1, vec![4], "test b.rs", false, "");
        tiles.subagent_ended(1, vec![5], "docs", true, "ok");
        assert_eq!(tiles.list.len(), 1, "one tile for the three");
        let tile = &tiles.list[0];
        assert_eq!(tile.subagents, 3);
        assert_eq!(tile.stand, Stand::Failed, "one broke off");
        assert_eq!(tile.lines, ["✓ review a.rs", "✗ test b.rs", "✓ docs"]);
        assert_eq!(
            tile.goes,
            Goes::Subagent {
                pane: 1,
                path: vec![5]
            }
        );
        // Another pane's are its own.
        tiles.subagent_ended(2, vec![1], "x", true, "y");
        assert_eq!(tiles.list.len(), 2);
        // A question stays; past the most, the oldest that does not ask goes.
        let mut asks = Tile::new(
            "reviewer".into(),
            Stand::Asks,
            "bash rm -rf target",
            Goes::Behind {
                name: "reviewer".into(),
                asks: true,
            },
        );
        asks.asks_of = Some(7);
        tiles.push(asks);
        tiles.push(Tile::new(
            "pane 2".into(),
            Stand::Done,
            "done",
            Goes::Pane(2),
        ));
        assert_eq!(tiles.list.len(), MOST);
        assert!(tiles.list.iter().any(|tile| tile.asks_of == Some(7)));
        assert!(
            !tiles.list.iter().any(|tile| tile.subagents == 3),
            "the oldest quiet one went"
        );
        tiles.tick(|_| true);
        assert_eq!(tiles.list.len(), MOST, "nothing stood its time yet");
        tiles.tick(|asker| asker != 7);
        assert!(
            tiles.list.iter().all(|tile| tile.asks_of.is_none()),
            "answered: it goes"
        );
        // Questions only: past the most, none goes.
        let mut questions = Tiles::default();
        for asker in 0..5 {
            let mut tile = Tile::new("pane".into(), Stand::Asks, "x", Goes::Pane(asker));
            tile.asks_of = Some(asker);
            questions.push(tile);
        }
        assert_eq!(questions.list.len(), 5);
    }

    #[test]
    fn tiles_stand_at_the_top_right_newest_first_clear_of_a_question_and_a_click_takes_one() {
        let _serial = serial();
        let mut tiles = Tiles::default();
        tiles.push(Tile::new("old".into(), Stand::Done, "first", Goes::Pane(1)));
        tiles.push(Tile::new(
            "new".into(),
            Stand::Failed,
            "second\u{202e}",
            Goes::Behind {
                name: "new".into(),
                asks: false,
            },
        ));
        let area = Rect::new(0, 0, 100, 40);
        let placed = tiles.place(area, &[]);
        assert_eq!(placed.len(), 2);
        assert_eq!(
            placed[0].0,
            Rect::new(59, 1, 40, 3),
            "the newest on top, at the right"
        );
        assert_eq!(placed[1].0.y, 4);
        let text: Vec<String> = placed[0].1.iter().map(ToString::to_string).collect();
        assert!(
            text[0].starts_with("╭─ ⬡ new") && text[0].ends_with("stopped ─╮"),
            "{text:?}"
        );
        assert!(
            text[1].starts_with("│ second ") && !text[1].contains('\u{202e}'),
            "{text:?}"
        );
        assert!(text[2].ends_with("click opens ─╯"), "{text:?}");
        use unicode_width::UnicodeWidthStr;
        assert!(text.iter().all(|row| row.width() == 40), "{text:?}");
        // Not over a question's card.
        let placed = tiles.place(area, &[Rect::new(50, 1, 50, 3)]);
        assert_eq!(placed.len(), 1);
        assert_eq!(tiles.clicked(60, 2), None, "the hidden one catches nothing");
        assert_eq!(tiles.clicked(60, 5), Some(Goes::Pane(1)));
        assert_eq!(tiles.list.len(), 1);
    }
}
