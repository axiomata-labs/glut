//! Several sessions in one window (`docs/plans/glut-tui.md`, T36a): the panes, each a whole session of its own — its
//! stream, input, model, mode and session file — sharing the terminal. Keys and pastes go to the active pane alone,
//! a click moves into the pane under it, the wheel goes to the pane under it; each session's reports reach its own
//! pane. A new pane is built beside the screen, so the others go on while its servers start.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event as TermEvent, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::app::App;
use crate::host::{Host, Launch, Servers};
use crate::session::{self, Report};
use crate::transcript::Tone;

/// The most panes on the screen (T36f): the main one and six beside it, two rows of three.
pub const MOST: usize = 7;
/// How many messages models send each other without the owner's word before the next waits for it (T36c).
pub const MOST_CHAIN: u32 = 6;
/// The most askers waiting to hear that a turn ended (T83, Q8).
const MOST_NOTIFY: usize = 16;
/// How many messages one session's model may send to other panes (T36c).
pub const MOST_TELLS: u32 = 30;
/// Background agents have no limit (T36f) — only the owner's `/bg` makes one —, but one beyond this many is asked for
/// once a window: each works, and costs, on its own.
pub const BACKGROUND_ASKED: usize = 6;
/// How long a first `/quit` or `/close` stays armed while a pane works: the second within it goes on.
const ARMED: Duration = Duration::from_secs(10);
/// The narrowest and the lowest a pane is let be beside the main one: below, the right side shows its panes as tabs.
const PANE_MIN_COLUMNS: u16 = 40;
const PANE_MIN_ROWS: u16 = 8;
/// The main pane's share of the width, in percent: where it starts, how far `⌃X {`/`⌃X }` move it, its bounds.
pub const SHARE: u16 = 50;
pub const SHARE_STEP: i16 = 5;
pub const SHARE_LEAST: u16 = 20;
pub const SHARE_MOST: u16 = 80;

/// How the panes stand (T36f): the main one — the pane Glut started with, or the one `/swap` put there — on the left at
/// full height, the others on the right in a grid of at most two rows: one at full height, two above each other, then
/// 2+1, 2+2, 3+2, 3+3.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Grid {
    /// The main pane, by its session's id.
    main: u64,
    /// The others in reading order: the top row left to right, then the bottom one.
    others: Vec<u64>,
    /// The main pane's share of the width, in percent.
    share: u16,
}

/// Where the panes stand in an area: each one's area (none for a pane shown only as a tab), the columns that part
/// panes side by side, and the tabs where the room is too small for all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Placed {
    pub areas: Vec<(u64, Rect)>,
    pub lines: Vec<Rect>,
    /// The column that parts the main pane from the others: the mouse moves it.
    pub border: Option<Rect>,
    pub tabs: Option<Tabs>,
}

/// Panes shown one at a time (T36f): a line of their tabs over the one shown whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs {
    /// The line the tabs stand on.
    pub area: Rect,
    /// The panes it holds, in reading order, by their sessions' ids.
    pub panes: Vec<u64>,
    /// The one shown.
    pub shown: u64,
}

impl Grid {
    fn new(main: u64) -> Self {
        Grid {
            main,
            others: Vec::new(),
            share: SHARE,
        }
    }

    /// The panes in reading order: the main one, then the grid's.
    fn order(&self) -> Vec<u64> {
        std::iter::once(self.main)
            .chain(self.others.iter().copied())
            .collect()
    }

    /// Takes pane `id` out; the main one gone, the first of the others takes its place.
    fn remove(&mut self, id: u64) {
        if id == self.main {
            if !self.others.is_empty() {
                self.main = self.others.remove(0);
            }
        } else {
            self.others.retain(|other| *other != id);
        }
    }

    /// Pane `id` and the main one trade places; `false` when `id` is none of the others.
    fn swap(&mut self, id: u64) -> bool {
        let Some(at) = self.others.iter().position(|other| *other == id) else {
            return false;
        };
        self.others[at] = self.main;
        self.main = id;
        true
    }

    /// How many of `count` panes stand in the grid's top row and how many in its bottom one.
    fn rows(count: usize) -> (usize, usize) {
        (count.div_ceil(2), count / 2)
    }

    /// The main pane's width in `width`: its share, kept so both it and the right side hold a pane.
    fn main_width(&self, width: u16) -> u16 {
        let share = u32::from(width) * u32::from(self.share) / 100;
        let most = width.saturating_sub(PANE_MIN_COLUMNS + 1);
        (share as u16).clamp(PANE_MIN_COLUMNS.min(most), most)
    }

    /// Whether `count` panes beside the main one keep each at least `least` columns and [`PANE_MIN_ROWS`] in `area`.
    fn fits(&self, area: Rect, count: usize, least: u16) -> bool {
        if count == 0 {
            return area.width >= least;
        }
        let main = self.main_width(area.width);
        let right = area.width.saturating_sub(main + 1);
        let (top, bottom) = Grid::rows(count);
        let columns = top as u16;
        let width = right.saturating_sub(columns - 1) / columns;
        let height = if bottom > 0 {
            area.height / 2
        } else {
            area.height
        };
        main >= least && width >= least && height >= PANE_MIN_ROWS
    }

    /// The narrowest this keeps every pane at least `least` columns wide.
    fn needs(&self, least: u16) -> u16 {
        let count = self.others.len();
        let tall = Rect::new(0, 0, 0, u16::MAX);
        // Every pane of the top row and the main one take `least` and a column each: no narrower width can do.
        let columns = Grid::rows(count).0 as u16;
        let floor = least.saturating_mul(columns + 1).saturating_add(columns);
        (floor..u16::MAX)
            .find(|width| {
                self.fits(
                    Rect {
                        width: *width,
                        ..tall
                    },
                    count,
                    least,
                )
            })
            .unwrap_or(u16::MAX)
    }

    /// Where each pane stands in `area`. A right side too small for its grid shows its panes as tabs — the active one
    /// whole when it is one of them, else `front`; a window too narrow for two panes shows every pane as a tab, the
    /// active one whole. The pane with the keys is always drawn.
    fn place(&self, area: Rect, front: u64, active: u64) -> Placed {
        let mut placed = Placed::default();
        if self.others.is_empty() {
            placed.areas.push((self.main, area));
            return placed;
        }
        if area.width < 2 * PANE_MIN_COLUMNS + 1 || area.height < PANE_MIN_ROWS {
            return Grid::tabbed(area, self.order(), active, placed);
        }
        let main = self.main_width(area.width);
        placed.areas.push((
            self.main,
            Rect {
                width: main,
                ..area
            },
        ));
        let border = Rect::new(area.x + main, area.y, 1, area.height);
        placed.lines.push(border);
        placed.border = Some(border);
        let right = Rect {
            x: area.x + main + 1,
            width: area.width - main - 1,
            ..area
        };
        if !self.fits(area, self.others.len(), PANE_MIN_COLUMNS) {
            let shown = [active, front]
                .into_iter()
                .find(|id| self.others.contains(id))
                .unwrap_or(self.others[0]);
            return Grid::tabbed(right, self.others.clone(), shown, placed);
        }
        let (top, bottom) = Grid::rows(self.others.len());
        let top_height = if bottom > 0 {
            right.height / 2
        } else {
            right.height
        };
        let rows = [
            (
                &self.others[..top],
                Rect {
                    height: top_height,
                    ..right
                },
            ),
            (
                &self.others[top..],
                Rect {
                    y: right.y + top_height,
                    height: right.height - top_height,
                    ..right
                },
            ),
        ];
        for (ids, row) in rows {
            let count = ids.len() as u16;
            for (at, id) in ids.iter().enumerate() {
                let at = at as u16;
                // One column between each two panes for the line that parts them.
                let width = row.width.saturating_sub(count - 1) / count;
                let x = row.x + at * (width + 1);
                if at > 0 {
                    placed.lines.push(Rect::new(x - 1, row.y, 1, row.height));
                }
                let width = if at + 1 == count {
                    row.width.saturating_sub(at * (width + 1))
                } else {
                    width
                };
                placed.areas.push((*id, Rect { x, width, ..row }));
            }
        }
        placed
    }

    /// `panes` in `area` as tabs: their line on top, `shown` whole below it, the others nowhere.
    fn tabbed(area: Rect, panes: Vec<u64>, shown: u64, mut placed: Placed) -> Placed {
        let line = Rect { height: 1, ..area };
        let below = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
        placed.areas.push((shown, below));
        placed.tabs = Some(Tabs {
            area: line,
            panes,
            shown,
        });
        placed
    }
}

/// The kept session `id` of `app`'s folder opened in it (TUI9e); one no longer kept — or never written, as a pane
/// that was never sent anything — is said, and the pane starts new.
fn open_kept(app: &mut App, id: &str) {
    if app.session.id == id {
        return;
    }
    if app.others_open.iter().any(|open| open == id) {
        return app.tell(tr!(kept_open_elsewhere));
    }
    match crate::store::Session::load(&app.parts.home, id) {
        Ok(session) if session.cwd == app.parts.config.cwd => app.open(session),
        _ => app.tell(tr!(kept_gone)),
    }
}

/// Where a new pane works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Asked in a git repository; this folder elsewhere.
    Ask,
    /// The active pane's folder.
    Here,
    /// A new worktree on this new branch.
    Tree(String),
    /// A worktree made before, found again (TUI9e).
    Kept(crate::worktree::Tree),
}

/// How a worktree's pane ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    /// Worktree and branch stay.
    Keep,
    /// The branch merged into the repository folder's, then worktree and branch removed.
    TakeOver,
    /// Worktree and branch removed — asked again when that loses work.
    Discard,
    /// Removed though it loses work: the owner's second yes.
    DiscardSure,
}

/// What a pane asks of the screen: its keys (`⌃]`, `⌃X 2`, `⌃X 0`, `⌃X {`), commands (`/split`, `/close`) and picks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A new pane in the grid (T36f) working at `place`, playing `role` (T36c); `resume`: an earlier session in it,
    /// picked from the list.
    Split {
        resume: bool,
        place: Place,
        role: Option<String>,
    },
    /// Close the asking pane (⌃X 0).
    Close,
    /// `/close` alone: close the asking pane — or, with others to close, pick which.
    CloseAsked,
    /// Close the pane or background agent of this session id, picked from `/close`'s list.
    ClosePane(u64),
    /// Close the asking pane, its worktree ending so.
    CloseAs(Leaving),
    /// The keys to the other pane.
    Toggle,
    /// A message to pane `to` — a number, a background agent's name or a role (T36e) — or to the only other one, taken
    /// there as if the owner typed it (T36b); `whole` is the text with that first word, sent as it is when it names
    /// no pane (`/send 3 files changed`).
    Send {
        to: Option<String>,
        text: String,
        whole: String,
    },
    /// `/send` alone: the panes, agents and other Glut windows to pick whom to write to.
    SendPick,
    /// The asking pane's last plan with its task list, to be built in pane `to`.
    SendPlan { to: Option<String> },
    /// A side question (T57) about pane or agent `to` — a word only when it names one, else the question's own, as
    /// `/send` reads it — or about the asking pane; `whole` is the text with that first word.
    Btw {
        to: Option<String>,
        question: String,
        whole: String,
    },
    /// A background agent (T36e): a session with no room on the screen, named `name` or after its role; `sure`: the
    /// owner said yes to one beyond [`BACKGROUND_ASKED`] (T36f).
    Background {
        name: Option<String>,
        role: Option<String>,
        place: Place,
        sure: bool,
    },
    /// The main pane and pane `n` (or the asking one) trade places (T36f, `/swap`).
    Swap(Option<String>),
    /// `/sessions` (T83): the other Glut windows to pick one to write to, or — `Some` — whether this one takes their
    /// messages.
    Sessions(Option<bool>),
    /// `/name` (T83): this window's name among the others, said, or set to the words given.
    Name(String),
    /// The main pane wider by this many percent of the window, or narrower (T36f, `⌃X }`, `⌃X {`).
    Widen(i16),
    /// A background agent brought forward as a pane.
    Show(String),
    /// The asking pane sent to the background.
    Hide,
    /// The panes kept at the last end, back as they stood (TUI9e, offered by `glut -c`).
    Restore,
    /// A loop in a background agent of its own (T60a).
    LoopStart(crate::looping::Asked),
    /// That loop ended, or the only one (`/loop stop`).
    LoopStop(Option<String>),
    /// The loops running listed (`/loop`).
    LoopList,
    /// A background agent's stream shown over the asking pane, only to read; `None` closes it.
    Peek(Option<String>),
    /// A background agent ended where it is, its turn stopped (`/close <name>`).
    End(String),
    /// Glut goes on in another project's folder (T44, `/projects`): asked first while it works.
    Switch(Switch),
    /// The switch the owner said yes to: the window ends as at a `/quit`, and Glut starts anew in the folder.
    SwitchNow(Switch),
    /// `/arena <models>: <task>` (T61): an arena of these models on this task.
    Arena(crate::arena::Asked),
    /// `/arena take <n>`: the arena's model n's changes into the owner's folder (counted from 0) — shown and asked
    /// first, `true` once the owner said yes.
    ArenaTake(usize, bool),
    /// `/arena end`: its panes and worktrees removed — asked first, `true` once the owner said yes.
    ArenaEnd(bool),
    /// `/arena card`: the comparison card again.
    ArenaCard,
    /// `/arena tasks <set>` (step 2): the set's tasks one after another on these models, each judged by its check.
    ArenaBench {
        set: String,
        contenders: Vec<crate::arena::Contender>,
    },
    /// `/arena check [command]`: the check command kept for this repository and run in every worktree — the one
    /// kept, without a command.
    ArenaCheck(Option<String>),
}

/// What a report is for a tile (T69), read before the pane takes it.
enum Tiled {
    /// A question of the gate, on this target.
    Asks(String),
    /// A turn ended — not one the owner stopped.
    Ended { answered: bool, answer: String },
    /// A subagent the turn started ended, by its `task` call.
    Subagent { id: String, ok: bool },
}

impl Tiled {
    fn of(report: &Report) -> Option<Tiled> {
        match report {
            Report::Ask(request, _) => Some(Tiled::Asks(request.target.clone())),
            Report::Form(asked, _) => Some(Tiled::Asks(format!("⬡ {}", asked.server))),
            Report::Done(outcome, _) if outcome.reason != glut::FinishReason::Cancelled => {
                Some(Tiled::Ended {
                    answered: outcome.reason == glut::FinishReason::Answered,
                    answer: outcome.answer.clone().unwrap_or_default(),
                })
            }
            Report::Event(glut::Event::ToolResult { id, name, ok, .. }) if name == "task" => {
                Some(Tiled::Subagent {
                    id: id.clone(),
                    ok: *ok,
                })
            }
            _ => None,
        }
    }
}

/// Where Glut starts anew (T44), and what it opens there: the folder's newest session, or one a search found (T55)
/// with the stream searched for its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub folder: std::path::PathBuf,
    pub session: Option<String>,
    pub find: Option<String>,
}

impl Switch {
    /// To `folder`, as `glut -c` there.
    pub fn to(folder: std::path::PathBuf) -> Self {
        Switch {
            folder,
            session: None,
            find: None,
        }
    }
}

/// `/send`'s words: maybe a pane first (a number, a name, a role — the screen knows which word names one), then the
/// text — or `plan`.
///
/// # Errors
///
/// What `/send` takes, when nothing is to be sent.
pub fn parse_send(rest: &str) -> Result<Request, String> {
    let rest = rest.trim();
    let empty = || Err(tr!(send_usage).to_owned());
    match rest.split_once(char::is_whitespace) {
        Some((first, text)) if text.trim() == "plan" => Ok(Request::SendPlan {
            to: Some(first.to_owned()),
        }),
        Some((first, text)) => Ok(Request::Send {
            to: Some(first.to_owned()),
            text: text.trim().to_owned(),
            whole: rest.to_owned(),
        }),
        None if rest == "plan" => Ok(Request::SendPlan { to: None }),
        // Nobody named: the panes, agents and windows to pick one from.
        None if rest.is_empty() => Ok(Request::SendPick),
        None if rest.parse::<u8>().is_ok() => empty(),
        None => Ok(Request::Send {
            to: None,
            text: rest.to_owned(),
            whole: rest.to_owned(),
        }),
    }
}

/// `/bg`'s words: a name, `as <role>`, and where — `here`, or `tree` with an optional branch name.
///
/// # Errors
///
/// What `/bg` takes, for a word it does not.
pub fn parse_background(rest: &str) -> Result<Request, String> {
    let mut name = None;
    let mut role = None;
    let mut place = Place::Ask;
    let mut words = rest.split_whitespace().peekable();
    while let Some(word) = words.next() {
        match word {
            "here" => place = Place::Here,
            "as" => match words.next() {
                Some(given) => role = Some(given.to_owned()),
                None => return Err(tr!(bg_as_usage).to_owned()),
            },
            "tree" => {
                let branch = words.next_if(|next| !matches!(*next, "here" | "tree" | "as"));
                place = Place::Tree(branch.unwrap_or_default().to_owned());
            }
            given if name.is_none() && is_name(given) => name = Some(given.to_owned()),
            other => {
                return Err(tr!(bg_unknown, word = other));
            }
        }
    }
    Ok(Request::Background {
        name,
        role,
        place,
        sure: false,
    })
}

/// Whether `word` can name a background agent: lower-case letters, digits and dashes, a letter first — never a pane's
/// number.
pub(crate) fn is_name(word: &str) -> bool {
    // `/send` and `/bg` read these words as their own.
    const RESERVED: [&str; 4] = ["plan", "here", "tree", "as"];
    !RESERVED.contains(&word)
        && word.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && word.len() <= 32
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `/split`'s words: `resume`, where — `here`, or `tree` with an optional branch name — and `as <role>`. A new pane
/// goes into the grid beside the main one (T36f): no side to name any more.
///
/// # Errors
///
/// What `/split` takes, for a word it does not.
pub fn parse_split(rest: &str) -> Result<Request, String> {
    let mut resume = false;
    let mut place = Place::Ask;
    let mut role = None;
    let mut words = rest.split_whitespace().peekable();
    while let Some(word) = words.next() {
        match word {
            "resume" => resume = true,
            "here" => place = Place::Here,
            "as" => match words.next() {
                Some(name) => role = Some(name.to_owned()),
                None => return Err(tr!(split_as_usage).to_owned()),
            },
            "tree" => {
                let branch =
                    words.next_if(|next| !matches!(*next, "resume" | "here" | "tree" | "as"));
                place = Place::Tree(branch.unwrap_or_default().to_owned());
            }
            "l" | "r" | "u" | "d" | "left" | "right" | "up" | "down" => {
                return Err(tr!(split_side, word = word));
            }
            other => {
                return Err(tr!(split_unknown, word = other));
            }
        }
    }
    Ok(Request::Split {
        resume,
        place,
        role,
    })
}

/// One pane: its number and its session.
pub struct Pane {
    /// Its session's own mark on the reports, never given twice — unlike the number a closed pane leaves free: a late
    /// report of a closed session must reach no pane.
    pub(crate) id: u64,
    /// The number the owner sees: the lowest free one when it opened.
    pub number: u8,
    /// A background agent's name (T36e); a pane sent to the background gets one, a pane brought forward keeps it.
    pub name: Option<String>,
    /// In the background: in no split, drawn nowhere, reached by its name.
    pub background: bool,
    pub app: App,
    /// Where it was drawn last: the mouse finds it there.
    pub area: Rect,
    /// The worktree it works in, made when it opened; `None` in the repository's own folder.
    pub tree: Option<crate::worktree::Tree>,
    /// The loop its agent runs (T60a).
    pub looping: Option<crate::looping::Loop>,
}

/// A pane whose session is still being built.
struct Building {
    id: u64,
    number: u8,
    resume: bool,
    tree: Option<crate::worktree::Tree>,
    role: Option<String>,
    /// The name of a background agent (T36e); `None` for a pane.
    background: Option<String>,
    /// The kept session it goes on with (TUI9e), by its id.
    restore: Option<String>,
    /// The loop it runs (T60a), with the folders beside the working one of the pane that started it.
    looping: Option<(crate::looping::Asked, Vec<glut::folders::Folder>)>,
    /// The arena's model it is the pane of (T61), by its place among the members.
    arena: Option<usize>,
    reports: mpsc::UnboundedSender<Report>,
}

/// A new pane's session as `session::build` makes it.
type Built = Result<(session::Parts, session::Engine), String>;

/// The panes on the screen.
pub struct Panes {
    /// The look's count of changes the panes were laid out for (TUI9b, `motion::look`).
    look: usize,
    /// The panes in the order they stand: left to right, or top to bottom.
    pub list: Vec<Pane>,
    /// The pane the keys go to, as a place in `list`.
    pub active: usize,
    /// How the panes stand.
    grid: Grid,
    /// The pane last shown whole beside the main one: the one the right side shows when it holds them as tabs.
    front: u64,
    /// Where the panes stood in the last frame: the room a new one would share, the tabs and the border a click finds.
    placed: Placed,
    /// The room the panes had in the last frame; empty before the first.
    room: Rect,
    /// The border to the main pane is being dragged by the mouse.
    dragging: bool,
    /// The owner said yes to more than [`BACKGROUND_ASKED`] agents in the background: not asked again this window.
    background_sure: bool,
    /// Whether this screen may split at all: a host's session (a card of Axiomata's studio) is its pane alone.
    may_split: bool,
    /// The first session's servers, which a new pane starts too — never more (`--strict-mcp-config` stays strict).
    servers: Servers,
    /// Every session's reports, with its id.
    reports: mpsc::UnboundedSender<(u64, Report)>,
    /// The next session's id.
    next_id: u64,
    building: Option<Building>,
    built: mpsc::UnboundedSender<Built>,
    /// A `/quit` said once while another pane works.
    quit_armed: Option<Instant>,
    /// A `/close` said once while the closing pane works: that pane's number, and when.
    close_armed: Option<(u8, Instant)>,
    /// The screen is drawn anew from blank: a session built beside it may have written to the terminal.
    pub redraw: bool,
    /// Messages between models since the owner's last word in any pane (T36c).
    chain: u32,
    /// A background agent's stream shown over the active pane, only to read (T36e, `/peek`), by its session's id.
    pub peek: Option<u64>,
    /// The first pane's folder: where the layout is kept (TUI9e).
    cwd: std::path::PathBuf,
    /// The kept layout coming back, pane by pane (TUI9e).
    restoring: Option<Restoring>,
    /// The kept session the next pane built goes on with.
    restore_next: Option<String>,
    /// A kept layout was offered (`glut -c`): a window ending with one pane forgets it.
    offered: bool,
    /// The links of the last frame, where they stand: a click on one opens it (the screen holds the mouse, so the
    /// terminal never sees that click).
    pub links: Vec<crate::links::Spot>,
    /// The project Glut goes on in once this window ended (T44): the screen's loop ends, the layout kept.
    pub switch_to: Option<Switch>,
    /// The window's focus and the notifications waiting to go out (T53).
    pub notifier: crate::notify::Notifier,
    /// What the environment says about the terminal, for the way a notification goes (T53).
    pub notify_env: crate::notify::Env,
    /// The tiles at the window's top right (T69).
    pub tiles: crate::tiles::Tiles,
    /// The left button went down on a tile: its release and drags are the tile's too.
    tile_pressed: bool,
    /// Where the look for the plans' ended runs stands (T60b); `None` before the first.
    runs_seen: Option<RunsSeen>,
    /// This window among the other Glut windows of the machine (T83); `None` where it talks to none.
    pub link: Option<Arc<crate::sessions::Link>>,
    /// `/sessions off`: nothing from outside comes in (Q11).
    pub sessions_off: bool,
    /// Who asked to hear when a pane's turn ends (Q8): the pane's session id and the asker's address.
    notify_back: Vec<(u64, String)>,
    /// The kept window name last tried, so one another window holds is not tried at every frame.
    window_tried: Option<String>,
    /// The pane the keys go back to once another one closed (`/close 2`): the owner stays where they were.
    return_to: Option<u64>,
    /// The model arena under way, or done and waiting for the owner's pick (T61).
    pub arena: Option<crate::arena::Arena>,
    /// The arena's comparison card over the screen, while open.
    pub card: Option<crate::arena::Card>,
    /// The test bench under way (T61 step 2): its arena is the task now run.
    pub bench: Option<crate::bench::Bench>,
}

/// How often an open Glut looks whether a plan's run ended (T60b): a tile for it, as for an agent's end.
pub const RUNS_LOOK: Duration = Duration::from_secs(15);

/// The runs told so far: those ended until `from`; the records' folder as it was then, read again only once it
/// changed.
struct RunsSeen {
    from: chrono::DateTime<chrono::Utc>,
    folder: Option<std::time::SystemTime>,
}

/// A kept layout coming back: the panes still to build, and each kept pane's session id once it stands.
struct Restoring {
    layout: crate::layout::Layout,
    /// The places in `layout.panes` still to build.
    left: std::collections::VecDeque<usize>,
    /// Each kept pane's id once built.
    ids: Vec<Option<u64>>,
    /// The place in `layout.panes` of the pane being built now.
    building: Option<usize>,
}

/// The other ends of [`Panes`]' channels, which the screen's loop waits on.
pub struct Inlets {
    /// Every session's reports, with its id.
    pub reports: mpsc::UnboundedReceiver<(u64, Report)>,
    /// A new pane's session, built — or why not.
    pub built: mpsc::UnboundedReceiver<Built>,
}

impl Panes {
    /// The first pane, its session's reports taken from `first` into the screen's.
    pub fn new(
        app: App,
        first: mpsc::UnboundedReceiver<Report>,
        may_split: bool,
        servers: Servers,
    ) -> (Self, Inlets) {
        let (reports, report_inlet) = mpsc::unbounded_channel();
        let (built, built_inlet) = mpsc::unbounded_channel();
        forward(1, first, reports.clone());
        let cwd = app.parts.config.cwd.clone();
        // The window's tiles as the owner left them (T69): once, as the window opens. Tests keep theirs.
        if !cfg!(test) {
            crate::tiles::set(
                crate::prefs::Prefs::load(&app.parts.home)
                    .tiles
                    .unwrap_or(true),
            );
        }
        // Tests do not guess from the terminal they happen to run in.
        let notify_env = if cfg!(test) {
            crate::notify::Env::default()
        } else {
            crate::notify::Env::read()
        };
        let panes = Panes {
            notifier: crate::notify::Notifier::new(&notify_env),
            tiles: crate::tiles::Tiles::default(),
            runs_seen: None,
            tile_pressed: false,
            notify_env,
            cwd,
            restoring: None,
            restore_next: None,
            offered: false,
            switch_to: None,
            links: Vec::new(),
            list: vec![Pane {
                looping: None,
                id: 1,
                number: 1,
                name: None,
                background: false,
                app,
                area: Rect::default(),
                tree: None,
            }],
            active: 0,
            grid: Grid::new(1),
            front: 1,
            placed: Placed::default(),
            room: Rect::default(),
            dragging: false,
            background_sure: false,
            may_split,
            servers,
            reports,
            next_id: 2,
            building: None,
            built,
            quit_armed: None,
            close_armed: None,
            redraw: false,
            chain: 0,
            peek: None,
            look: crate::motion::look(),
            link: None,
            sessions_off: false,
            notify_back: Vec::new(),
            window_tried: None,
            return_to: None,
            arena: None,
            card: None,
            bench: None,
        };
        (
            panes,
            Inlets {
                reports: report_inlet,
                built: built_inlet,
            },
        )
    }

    /// The active pane's session.
    pub fn active(&mut self) -> &mut App {
        &mut self.list[self.active].app
    }

    /// Whether more than one pane stands on the screen.
    pub fn split(&self) -> bool {
        self.visible() > 1
    }

    /// How many panes stand on the screen (background agents apart).
    pub fn visible(&self) -> usize {
        self.list.iter().filter(|pane| !pane.background).count()
    }

    /// The pane a word names, other than `except`: its number, a background agent's name, or its role (T36e).
    /// A name wins over a number and a number over a role; a role two panes play names neither.
    fn resolve(&self, word: &str, except: usize) -> Option<usize> {
        let others = || (0..self.list.len()).filter(move |at| *at != except);
        if let Some(at) = others().find(|at| self.list[*at].name.as_deref() == Some(word)) {
            return Some(at);
        }
        if let Ok(number) = word.parse::<u8>() {
            return others().find(|at| self.list[*at].number == number);
        }
        let playing: Vec<usize> = others()
            .filter(|at| self.list[*at].app.role.as_deref() == Some(word))
            .collect();
        (playing.len() == 1).then(|| playing[0])
    }

    /// The background agent a word names: its name, else the role only one of them plays.
    fn resolve_background(&self, word: &str) -> Option<usize> {
        let behind = || (0..self.list.len()).filter(|at| self.list[*at].background);
        if let Some(at) = behind().find(|at| self.list[*at].name.as_deref() == Some(word)) {
            return Some(at);
        }
        let playing: Vec<usize> = behind()
            .filter(|at| self.list[*at].app.role.as_deref() == Some(word))
            .collect();
        (playing.len() == 1).then(|| playing[0])
    }

    /// How a pane is named to the models, always English: "pane 2", or a background agent's name.
    fn label(&self, at: usize) -> String {
        let pane = &self.list[at];
        match (&pane.name, pane.background) {
            (Some(name), true) => name.clone(),
            _ => format!("pane {}", pane.number),
        }
    }

    /// How a pane is named to the owner, in the language read now: "Pane 2", or a background agent's name.
    fn shown_label(&self, at: usize) -> String {
        let pane = &self.list[at];
        match (&pane.name, pane.background) {
            (Some(name), true) => name.clone(),
            _ => tr!(pane_named, n = pane.number),
        }
    }

    /// The one other pane or agent when there is exactly one, for a message that names none.
    fn only_other(&self, except: usize) -> Option<usize> {
        let others: Vec<usize> = (0..self.list.len()).filter(|at| *at != except).collect();
        (others.len() == 1).then(|| others[0])
    }

    /// Once per frame: answered questions go, and what the panes share is told to each — the terminal's mouse (the
    /// active pane's word is the screen's) and the sessions the others hold.
    pub fn sync(&mut self) {
        self.sync_windows();
        // A question's tile stands while its session waits and its card is not the active pane's; the others their
        // time (T69).
        let active = self.list[self.active].id;
        let waiting: Vec<u64> = self
            .list
            .iter()
            .filter(|pane| pane.app.waits_for_owner() && pane.id != active)
            .map(|pane| pane.id)
            .collect();
        self.tiles.tick(|id| waiting.contains(&id));
        // The look changed (`/motion`, `/reader`): every pane's blocks are laid out anew, its pictures from their
        // first frame (TUI9b).
        let look = crate::motion::look();
        if std::mem::replace(&mut self.look, look) != look {
            // Drawn whole from blank: a cell that only lost its link (`/links plain`) is no change to ratatui.
            self.redraw = true;
            for pane in &mut self.list {
                pane.app.frame = 0;
                pane.app.transcript.touch_all();
            }
        }
        let held = self.list[self.active].app.mouse_held;
        let numbers: Vec<u8> = self.list.iter().map(|pane| pane.number).collect();
        // Only a pane on the screen is "the other pane" a plan is built in (T36e).
        let on_screen: Vec<u8> = self
            .list
            .iter()
            .filter(|pane| !pane.background)
            .map(|pane| pane.number)
            .collect();
        let open: Vec<String> = self
            .list
            .iter()
            .map(|pane| pane.app.session.id.clone())
            .collect();
        // What `tell` says of each pane to the others (T36c).
        let described: Vec<(u8, String)> = self
            .list
            .iter()
            .map(|pane| {
                let app = &pane.app;
                let mode = session::mode_label(app.mode());
                let model = format!("{} ({})", app.engine.model_name(), app.engine.provider);
                (
                    pane.number,
                    format!(
                        "{}: {}, {}, {mode} mode, in {}",
                        match (&pane.name, pane.background) {
                            (Some(name), true) => {
                                format!("{name} (pane {}, an agent in the background)", pane.number)
                            }
                            _ => format!("pane {}", pane.number),
                        },
                        app.role.as_deref().unwrap_or("all-rounder"),
                        glut::permissions::printable(&model),
                        crate::transcript::cut(&glut::permissions::printable(&app.folder), 80)
                    ),
                )
            })
            .collect();
        // The Agents tab of every pane lists the agents in the background (T36e), each a card (T69).
        let background: Vec<crate::agents::Behind> = self
            .list
            .iter()
            .filter(|pane| pane.background)
            .map(|pane| {
                use crate::agents::Doing;
                let app = &pane.app;
                let doing = if app.waits_for_owner() {
                    Doing::Asks
                } else if !app.held.is_empty() {
                    Doing::Holds
                } else if app.working() {
                    Doing::Works
                } else {
                    Doing::Rests
                };
                let cache = app
                    .cache_seen
                    .then(|| app.warm_until.is_some_and(|until| until > Instant::now()));
                crate::agents::Behind {
                    name: pane.name.clone().unwrap_or_else(|| "agent".to_owned()),
                    number: pane.number,
                    role: app.role.clone().unwrap_or_else(|| "all-rounder".to_owned()),
                    model: app.engine.model_name().to_owned(),
                    doing,
                    live: app.live_line(),
                    cost: app.cost,
                    cache,
                    working: app.working_since.map(|since| since.elapsed()),
                    looping: pane.looping.as_ref().map(|looping| {
                        let mut line = format!(
                            "↻ {} {}/{}",
                            crate::looping::shown(looping.asked.every),
                            looping.rounds,
                            looping.limits.rounds
                        );
                        // On a subscription its tokens are what it spends: the card says how far it got.
                        if let Some(max) = looping.limits.max_tokens {
                            let used = format!(
                                "{}/{}",
                                crate::looping::tokens_shown(looping.used),
                                crate::looping::tokens_shown(max)
                            );
                            line.push_str(" · ");
                            line.push_str(&tr!(loop_tokens_used, n = used));
                        }
                        line
                    }),
                    looped: pane.app.loop_ended.clone(),
                }
            })
            .collect();
        if self
            .list
            .iter_mut()
            .any(|pane| std::mem::take(&mut pane.app.owner_spoke))
        {
            self.chain = 0;
        }
        for (at, pane) in self.list.iter_mut().enumerate() {
            pane.app.drop_answered_ask();
            pane.app.track_state();
            pane.app.background = background.clone();
            pane.app.parts.pane_link.set(
                pane.number,
                described
                    .iter()
                    .filter(|(number, _)| *number != pane.number)
                    .map(|(_, line)| line.clone())
                    .collect(),
            );
            pane.app.mouse_held = held;
            pane.app.others = numbers
                .iter()
                .copied()
                .filter(|number| *number != pane.number && on_screen.contains(number))
                .collect();
            pane.app.others_open = open
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != at)
                .map(|(_, id)| id.clone())
                .collect();
        }
    }

    /// A report of session `id`; one of a pane closed since is nobody's.
    pub fn on_report(&mut self, id: u64, report: Report) {
        // A model's message to another pane is the screen's to deliver (T36c).
        if let Report::Tell {
            to,
            text,
            notify,
            reply,
        } = report
        {
            // A word no pane here answers to, but another window's name (T83): the message goes there.
            if let Some(source) = self.list.iter().position(|pane| pane.id == id)
                && let Some(address) = to
                    .as_deref()
                    .filter(|word| self.resolve(word, source).is_none())
                    .filter(|word| self.reaches_window(word))
            {
                return self.tell_window(source, address.to_owned(), text, notify, reply);
            }
            let _ = reply.send(self.tell(id, to, text));
            return;
        }
        // A turn asked about by another window ended (Q8): it hears so, once.
        if matches!(report, Report::Done(..)) {
            self.notify_done(id);
        }
        let Some(at) = self.list.iter().position(|pane| pane.id == id) else {
            return;
        };
        // An arena's model whose turn ended is done with the task (T61); one a limit stopped stays aborted. Its
        // refusals count either way.
        if let Report::Done(outcome, _) = &report
            && let Some(member) = self.arena.as_mut().and_then(|arena| arena.member_of(id))
        {
            member.refusals += outcome.refusals;
            if member.state == crate::arena::State::Working {
                member.state = crate::arena::State::Done(outcome.reason);
                member.took = member.began.map(|began| began.elapsed());
            }
        }
        // An arena's check ended in a model's worktree.
        // Only the check last started there: a slower earlier one ends unheard.
        if let Report::Shell { id: shell, ran, .. } = &report
            && let Some(arena) = &mut self.arena
            && let Some(at) = arena
                .members
                .iter()
                .position(|member| member.pane == Some(id))
            && arena.check_ids[at].as_ref() == Some(shell)
        {
            arena.check_ids[at] = None;
            // Kept in the results: masked as any tool text is, since a model wrote the code the check ran.
            let masks = self
                .list
                .iter()
                .find(|pane| pane.id == id)
                .and_then(|pane| pane.app.parts.config.masks.clone());
            arena.check_said[at] = (!ran.ok).then(|| {
                let reason = crate::arena::check_reason(&ran.output);
                match &masks {
                    Some(masks) => masks.mask(&reason, "arena check").text,
                    None => reason,
                }
            });
            arena.checks[at] = if ran.ok {
                crate::arena::Check::Passed
            } else {
                crate::arena::Check::Failed
            };
        }
        // A turn the host refused before it began (a hook's veto) never ends with a `Done`.
        if let Report::Refused { reason, .. } = &report
            && let Some(member) = self.arena.as_mut().and_then(|arena| arena.member_of(id))
            && member.state == crate::arena::State::Working
        {
            member.state = crate::arena::State::Aborted(crate::transcript::clean(reason));
            member.took = member.began.map(|began| began.elapsed());
        }
        let occasion = self.occasion(at, &report);
        let tile = Tiled::of(&report);
        // How a loop's round ended is the loop's to judge once its agent rests (T60a).
        if let Some(looping) = &mut self.list[at].looping {
            match &report {
                Report::Done(outcome, _) => {
                    looping.last =
                        Some((outcome.reason, outcome.answer.clone().unwrap_or_default()));
                }
                // The host refused the turn before it began: a round that cannot run.
                Report::Refused { reason, .. } => {
                    looping.last = Some((glut::FinishReason::Refused, reason.clone()));
                }
                _ => {}
            }
        }
        self.list[at].app.on_report(report);
        if let Some((occasion, ran)) = occasion {
            self.notice(at, &occasion, ran);
        }
        if let Some(tile) = tile {
            self.tile(at, tile);
        }
    }

    /// Whether the active pane shows its Agents tab: what it lists needs no tile.
    fn agents_shown(&self) -> bool {
        let app = &self.list[self.active].app;
        app.panel == Some(true) && app.panel_drawn && app.tab == crate::app::Tab::Agents
    }

    /// A tile for what the pane at `at` reported (T69), unless the owner sees it already: a question of any pane but
    /// the active one; the end of a background agent's turn, or of a pane drawn nowhere (held as a tab); a subagent of
    /// a pane on the screen that ended.
    fn tile(&mut self, at: usize, tiled: Tiled) {
        use crate::tiles::{Goes, Stand, Tile};
        let pane = &self.list[at];
        // A loop's round that ended is told only when the owner asked for it (T60a): its end is the loop's.
        let quiet_loop = pane
            .looping
            .as_ref()
            .is_some_and(|looping| !looping.asked.notify);
        if quiet_loop && matches!(tiled, Tiled::Ended { .. }) {
            return;
        }
        let who = self.shown_label(at);
        let goes = match (&pane.name, pane.background) {
            (Some(name), true) => Goes::Behind {
                name: name.clone(),
                asks: matches!(tiled, Tiled::Asks(_)),
            },
            _ => Goes::Pane(pane.id),
        };
        match tiled {
            Tiled::Asks(target) if at != self.active => {
                let mut tile = Tile::new(who, Stand::Asks, &target, goes);
                tile.asks_of = Some(pane.id);
                self.tiles.push(tile);
            }
            Tiled::Asks(_) => {}
            // A queued message started the next turn: the work is not done.
            Tiled::Ended { .. } if pane.app.working() => {}
            Tiled::Ended { answered, answer } => {
                let unseen = if pane.background {
                    !self.agents_shown()
                } else {
                    at != self.active && pane.area.is_empty()
                };
                if unseen {
                    let (stand, line) = if answered {
                        (Stand::Done, answer)
                    } else {
                        (Stand::Failed, String::new())
                    };
                    self.tiles.push(Tile::new(who, stand, &line, goes));
                }
            }
            Tiled::Subagent { id, ok } => {
                // One the owner stopped with the turn (Esc) ended as they wanted it to.
                if pane.background
                    || pane.app.stopping()
                    || (at == self.active && self.agents_shown())
                {
                    return;
                }
                let found = pane
                    .app
                    .transcript
                    .agents()
                    .into_iter()
                    .find(|(path, agent)| path.len() == 1 && agent.id == id)
                    .map(|(path, agent)| (path, agent.task.clone(), agent.answer.clone()));
                if let Some((path, task, answer)) = found {
                    let id = pane.id;
                    self.tiles.subagent_ended(id, path, &task, ok, &answer);
                }
            }
        }
    }

    /// What `report` of the pane at `at` is for a notification (T53), and how long its turn has run: an ask, or the end
    /// of a turn the owner did not stop.
    fn occasion(&self, at: usize, report: &Report) -> Option<(crate::notify::Occasion, Duration)> {
        let app = &self.list[at].app;
        let ran = app
            .working_since
            .or(app.busy_since)
            .map(|since| since.elapsed())
            .unwrap_or_default();
        let occasion = match report {
            Report::Ask(request, _) => crate::notify::Occasion::Asks {
                tool: request.tool.clone(),
            },
            // Named by its server alone: what the form asks may be the owner's to keep.
            Report::Form(asked, _) => crate::notify::Occasion::Asks {
                tool: glut::mcp::tools::model_name(&asked.server, ""),
            },
            Report::Done(outcome, _) if outcome.reason != glut::FinishReason::Cancelled => {
                crate::notify::Occasion::Ended {
                    ran,
                    answered: outcome.reason == glut::FinishReason::Answered,
                }
            }
            _ => return None,
        };
        Some((occasion, ran))
    }

    /// Tells the owner of `occasion` of the pane at `at` (T53): a message while the window is elsewhere, a toast in the
    /// active pane when another one ends while the owner looks — an ask elsewhere shows in the footer already.
    fn notice(&mut self, at: usize, occasion: &crate::notify::Occasion, ran: Duration) {
        use glut::permissions::settings::NotifyWhen;
        let choice = crate::notify::choice();
        // A queued message started the next turn: the work is not done. A loop's round is told as its tile is.
        let quiet_loop = self.list[at]
            .looping
            .as_ref()
            .is_some_and(|looping| !looping.asked.notify);
        if !occasion.is_ask() && (self.list[at].app.working() || quiet_loop) {
            return;
        }
        let who = (self.list.len() > 1).then(|| {
            let pane = &self.list[at];
            match (&pane.name, pane.background) {
                (Some(name), true) => name.clone(),
                _ => tr!(pane_named, n = pane.number),
            }
        });
        if self.notifier.focused == Some(true) {
            let long = ran >= Duration::from_secs(u64::from(choice.after));
            if choice.when != NotifyWhen::Off
                && choice.when != NotifyWhen::Asks
                && at != self.active
                && !occasion.is_ask()
                && long
            {
                let said = format!("{}: {}", who.unwrap_or_default(), occasion.said());
                self.active().say(said);
            }
            return;
        }
        if crate::notify::worth_a_message(choice, self.notifier.focused, occasion, ran) {
            self.notifier.push(who, occasion);
        }
    }

    /// The window's project as a notification names it: its folder's name.
    pub fn project_name(&self) -> String {
        let project = &self.list[self.active].app.project;
        project.file_name().map_or_else(
            || project.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// Sends what waits to be told, if it may go now (T53).
    pub fn send_notifications(&mut self, out: &mut impl std::io::Write) {
        let project = self.project_name();
        if let Some(text) = self.notifier.take(&project, Instant::now()) {
            let way = self.notify_env.way(crate::notify::choice().when);
            // A terminal that cannot take the message takes no frame either: the loop says so on its next draw.
            let _ = crate::notify::send(out, way, &text, self.notify_env.tmux);
        }
    }

    /// Session `id`'s model writes to pane `to` (a number, a background agent's name, a role; the only other one when
    /// none): delivered as a message of the other model, never the owner's — held for the owner's Enter once the models
    /// have written [`MOST_CHAIN`] times without the owner's word, refused past [`MOST_TELLS`]. What it became, for the
    /// model.
    fn tell(&mut self, id: u64, to: Option<String>, text: String) -> Result<String, String> {
        let source = self
            .list
            .iter()
            .position(|pane| pane.id == id)
            .ok_or("this pane is closed")?;
        let from = self.list[source].number;
        let target = match &to {
            Some(word) => self.resolve(word, source).ok_or(format!(
                "there is no pane or agent {}",
                crate::transcript::clean(word)
            ))?,
            None => match self.only_other(source) {
                Some(at) => at,
                None if self.list.len() == 1 => return Err("there is no other pane".into()),
                None => return Err("name the pane or agent: there are several".into()),
            },
        };
        if self.list[source].app.tells_sent >= MOST_TELLS {
            return Err(format!(
                "this session has sent {MOST_TELLS} messages to other panes: no more — tell the owner instead"
            ));
        }
        self.list[source].app.tells_sent += 1;
        self.chain += 1;
        let label = self.label(target);
        let message = crate::tell::wrapped(from, &text);
        // A model may start a turn only in a pane that can do no more than its own: no looser mode, no "always" it
        // lacks, and the same folder — a pane elsewhere (a worktree, the repository) writes where the sender cannot —
        // unless it only plans. Else, and past the chain, it waits for the owner's Enter there, its text shown.
        let sender = self.list[source].app.sender();
        let target_app = &self.list[target].app;
        let same_folder = target_app.parts.config.cwd == self.list[source].app.parts.config.cwd;
        let only_plans = target_app.mode() == glut::permissions::Mode::Plan;
        let wider = !target_app.no_wider_than(&sender) || !(same_folder || only_plans);
        let may_start = !wider && self.chain <= MOST_CHAIN;
        use crate::app::Delivered;
        let delivered = self.list[target].app.deliver_model(
            message,
            may_start,
            sender,
            &tr!(held_enter, pane = from),
        )?;
        Ok(match delivered {
            Delivered::Now => format!("delivered: {label} starts on it now"),
            Delivered::AfterTurn => format!("delivered: {label} reads it after its current turn"),
            Delivered::WhenFree => format!("delivered: {label} reads it once it is free"),
            Delivered::OnEnter if wider => format!(
                "held: {label} can do more than this pane (its mode, its rules or its folder), so it reads it once the \
                 owner says so there"
            ),
            Delivered::OnEnter if !may_start => format!(
                "held: the panes have written to each other often without the owner — {label} reads it once the \
                 owner says so"
            ),
            Delivered::OnEnter => format!("held: {label} reads it once the owner goes on there"),
        })
    }

    /// `/send` alone: whom to write to — the other panes and agents of this window, then the other windows' panes as
    /// last seen; ⏎ puts `/send <who> ` into the input, the message to be written after it.
    fn send_picker(&mut self) {
        use crate::picker::{Item, Pick, Picker};
        let mut items: Vec<Item> = (0..self.list.len())
            .filter(|at| *at != self.active)
            .map(|at| {
                let pane = &self.list[at];
                let who = match &pane.name {
                    Some(name) if pane.background => name.clone(),
                    _ => pane.number.to_string(),
                };
                let detail = format!(
                    "{} · {} · {}",
                    tr!(send_here),
                    pane.app.role.as_deref().unwrap_or("all-rounder"),
                    crate::transcript::clean(pane.app.engine.model_name())
                );
                Item::plain(self.shown_label(at), detail, Pick::WriteTo(who))
            })
            .collect();
        if let Some(link) = &self.link {
            items.extend(
                link.seen()
                    .into_iter()
                    .filter(|card| !card.off)
                    .flat_map(|card| card.seats)
                    .map(|seat| {
                        let address = crate::transcript::clean(&seat.address);
                        let detail = format!(
                            "{} · {} · {}",
                            crate::transcript::cut(&crate::transcript::clean(&seat.project), 40),
                            crate::transcript::clean(&seat.model),
                            crate::transcript::clean(&seat.mode)
                        );
                        Item::plain(address.clone(), detail, Pick::WriteTo(address))
                    }),
            );
        }
        if items.is_empty() {
            return self.active().tell(tr!(one_pane_send));
        }
        self.active().picker = Some(Picker::new(tr!(send_title), items, None));
    }

    /// `/sessions` (T83): every pane of the other windows as last seen, ⏎ writes to one; they are looked at anew
    /// beside the screen for the next time.
    fn sessions_picker(&mut self) {
        use crate::picker::{Item, Pick, Picker};
        let Some(link) = self.link.clone() else {
            return self.active().say(tr!(sessions_unlinked));
        };
        let items: Vec<Item> = link
            .seen()
            .into_iter()
            .filter(|card| !card.off)
            .flat_map(|card| card.seats)
            .map(|seat| {
                let detail = format!(
                    "{} · {} · {} · {}",
                    crate::transcript::cut(&crate::transcript::clean(&seat.project), 40),
                    crate::transcript::clean(&seat.model),
                    crate::transcript::clean(&seat.mode),
                    if seat.works {
                        tr!(sessions_works)
                    } else {
                        tr!(sessions_waits)
                    }
                );
                let address = crate::transcript::clean(&seat.address);
                Item::plain(address.clone(), detail, Pick::WriteTo(address))
            })
            .collect();
        tokio::spawn(async move {
            link.look().await;
        });
        if items.is_empty() {
            return self.active().say(tr!(windows_none));
        }
        self.active().picker = Some(Picker::new(tr!(windows_title), items, None));
    }

    /// `/name` (T83): this window's name said, or changed — kept with the main pane's session (Q13).
    fn name_window(&mut self, name: &str) {
        let Some(link) = self.link.clone() else {
            return self.active().say(tr!(sessions_unlinked));
        };
        if name.is_empty() {
            return self.active().say(tr!(name_now, name = link.name()));
        }
        match link.rename(name) {
            Ok(()) => {
                self.list[0].app.session.window = Some(name.to_owned());
                self.list[0].app.keep();
                self.active().say(tr!(name_set, name = name));
            }
            Err(reason) => self.active().say(reason),
        }
    }

    /// Whether `word`, naming no pane here, is meant for another window: one seen, or an address of a window's form
    /// (`name-xy`, `name/2`) — a window started since the last look is reached too, and one that is none says so.
    fn reaches_window(&self, word: &str) -> bool {
        self.link.as_ref().is_some_and(|link| {
            link.names_other(word)
                || (crate::sessions::check_address(word).is_ok()
                    && (word.contains('/') || word.contains('-')))
        })
    }

    /// This window's address of the pane at `at` (T83): its name for the first pane, else `name/<n>` or the agent's
    /// name behind it.
    fn address(&self, at: usize) -> String {
        let window = self
            .link
            .as_ref()
            .map_or_else(String::new, |link| link.name());
        let pane = &self.list[at];
        match &pane.name {
            Some(name) if pane.background => format!("{window}/{name}"),
            // The main pane — the first of the list, as `/swap` keeps it — answers to the window's name alone.
            _ if at == 0 => window,
            _ => format!("{window}/{}", pane.number),
        }
    }

    /// Session `id`'s model writes to another window (T83): counted with its messages to the panes here, sent beside
    /// the screen, its answer to the model once the other window gave it.
    fn tell_window(
        &mut self,
        source: usize,
        address: String,
        text: String,
        notify: bool,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
    ) {
        let Some(link) = self.link.clone() else {
            let _ = reply.send(Err("this window talks to no other".into()));
            return;
        };
        if self.list[source].app.tells_sent >= MOST_TELLS {
            let _ = reply.send(Err(format!(
                "this session has sent {MOST_TELLS} messages: no more — tell the owner instead"
            )));
            return;
        }
        self.list[source].app.tells_sent += 1;
        self.chain += 1;
        let letter = crate::sessions::Letter {
            from: self.address(source),
            to: String::new(),
            text,
            by: crate::sessions::By::Model,
            mode: self.list[source].app.mode().name().to_owned(),
            notify,
            chain: self.chain,
            rights: Some(crate::sessions::Rights {
                sender: self.list[source].app.sender(),
                folder: self.list[source].app.parts.config.cwd.clone(),
            }),
        };
        tokio::spawn(async move {
            let _ = reply.send(link.send(&address, letter).await);
        });
    }

    /// The owner's `/send` to another window (T83): it goes in at once there; what became of it is said here.
    fn send_window(&mut self, address: String, text: String) {
        let Some(link) = self.link.clone() else {
            return;
        };
        let letter = crate::sessions::Letter {
            from: self.address(self.active),
            to: String::new(),
            text,
            by: crate::sessions::By::Owner,
            mode: self.active().mode().name().to_owned(),
            notify: false,
            chain: 0,
            rights: None,
        };
        let id = self.list[self.active].id;
        let reports = self.reports.clone();
        tokio::spawn(async move {
            let said = match link.send(&address, letter).await {
                Ok(how) => tr!(sent_window, address = address, how = how),
                Err(reason) => tr!(sent_window_failed, address = address, reason = reason),
            };
            let _ = reports.send((id, Report::Host(crate::host::Signal::Note(said))));
        });
    }

    /// The pane of session `id` ended a turn: whoever asked to hear of it hears it now (Q8).
    fn notify_done(&mut self, id: u64) {
        let Some(link) = self.link.clone() else {
            return;
        };
        let Some(at) = self.list.iter().position(|pane| pane.id == id) else {
            return;
        };
        let from = self.address(at);
        let mode = self.list[at].app.mode().name().to_owned();
        let asked: Vec<String> = self
            .notify_back
            .iter()
            .filter(|(pane, _)| *pane == id)
            .map(|(_, address)| address.clone())
            .collect();
        self.notify_back.retain(|(pane, _)| *pane != id);
        for address in asked {
            let letter = crate::sessions::Letter {
                from: from.clone(),
                to: String::new(),
                text: format!("{from} ended the turn your message started."),
                by: crate::sessions::By::Notice,
                mode: mode.clone(),
                notify: false,
                chain: self.chain,
                rights: Some(crate::sessions::Rights {
                    sender: self.list[at].app.sender(),
                    folder: self.list[at].app.parts.config.cwd.clone(),
                }),
            };
            let link = Arc::clone(&link);
            tokio::spawn(async move {
                let _ = link.send(&address, letter).await;
            });
        }
    }

    /// What another window asks (T83): who this one is, or to take a message — the owner's in at once, a model's in at
    /// once only from the same mode and within the chain, else held for the owner's Enter (Q6, Q7).
    pub fn on_incoming(&mut self, incoming: crate::sessions::Incoming) {
        use crate::sessions::{Ask, Card, Reply, Seat};
        let reply = match incoming.ask {
            Ask::Who => Reply {
                card: Some(Card {
                    name: self
                        .link
                        .as_ref()
                        .map_or_else(String::new, |link| link.name()),
                    off: self.sessions_off,
                    // A window that takes nothing from outside tells nothing of its panes either.
                    seats: (0..self.list.len())
                        .filter(|_| !self.sessions_off)
                        .map(|at| {
                            let app = &self.list[at].app;
                            Seat {
                                address: self.address(at),
                                project: app.parts.config.cwd.display().to_string(),
                                model: app.engine.model_name().to_owned(),
                                mode: app.mode().name().to_owned(),
                                role: app.role.clone(),
                                works: app.working(),
                            }
                        })
                        .collect(),
                }),
                sent: None,
            },
            Ask::Send(letter) => Reply {
                card: None,
                sent: Some(self.take_letter(letter)),
            },
        };
        let _ = incoming.reply.send(reply);
    }

    /// A message from another window, delivered to the pane it names.
    fn take_letter(&mut self, letter: crate::sessions::Letter) -> Result<String, String> {
        use crate::sessions::By;
        if self.sessions_off {
            return Err("this Glut takes no messages from other windows".into());
        }
        if letter.text.trim().is_empty() || letter.text.len() > crate::tell::MOST_BYTES {
            return Err("an empty message, or one too long".into());
        }
        if crate::sessions::check_address(&letter.from).is_err() {
            return Err("a sender of no Glut window's address".into());
        }
        let target = if letter.to.is_empty() {
            Some(0)
        } else {
            self.resolve(&letter.to, usize::MAX)
        }
        .ok_or_else(|| format!("there is no pane {}", crate::transcript::clean(&letter.to)))?;
        let from = crate::transcript::cut(&crate::transcript::clean(&letter.from), 60);
        let message = crate::tell::wrapped_session(&from, letter.by, &letter.text);
        let label = self.address(target);
        use crate::app::Delivered;
        let delivered = match letter.by {
            // The owner's own word in another window goes in at once (Q6).
            By::Owner => {
                if self.list[target].app.queue.len() >= crate::app::MOST_HELD {
                    return Err("messages wait there already: try again later".into());
                }
                self.list[target].app.deliver(message)
            }
            By::Model | By::Notice => {
                // The sender counted this message already (Q7): the chain is the longer of the two.
                self.chain = self.chain.max(letter.chain);
                let theirs = letter.mode.as_str();
                let target_app = &self.list[target].app;
                let ours = target_app.mode().name();
                let same_mode = theirs == ours;
                // As `tell` between the panes of one window (Q6b): a pane that can do more than the sender — a looser
                // mode, an "always" it lacked, a wider role, another folder unless it only plans — starts nothing on
                // its word. A letter without the sender's rights is taken for one from a pane that may do nothing.
                let wider = match &letter.rights {
                    Some(rights) => {
                        let same_folder = target_app.parts.config.cwd == rights.folder;
                        let only_plans = target_app.mode() == glut::permissions::Mode::Plan;
                        !target_app.no_wider_than(&rights.sender) || !(same_folder || only_plans)
                    }
                    None => true,
                };
                let may_start = same_mode && !wider && self.chain <= MOST_CHAIN;
                let why = if !same_mode {
                    tr!(
                        held_window_mode,
                        from = from,
                        theirs = crate::transcript::clean(theirs),
                        ours = ours
                    )
                } else if wider {
                    tr!(held_window_wider, from = from)
                } else {
                    tr!(held_window_chain, from = from)
                };
                // The sender's own rights go with a message waiting for a turn's end, checked again then.
                let sender = letter
                    .rights
                    .as_ref()
                    .map_or_else(crate::app::Sender::nothing, |rights| rights.sender.clone());
                let delivered = self.list[target]
                    .app
                    .deliver_model(message, may_start, sender, &why)?;
                // Only a turn the message started is told of (Q8), and only a few askers are kept.
                if letter.notify
                    && letter.by == By::Model
                    && delivered == Delivered::Now
                    && self.notify_back.len() < MOST_NOTIFY
                {
                    let id = self.list[target].id;
                    self.notify_back.push((id, letter.from.clone()));
                }
                delivered
            }
        };
        Ok(match delivered {
            Delivered::Now => format!("delivered: {label} starts on it now"),
            Delivered::AfterTurn => format!("delivered: {label} reads it after its current turn"),
            Delivered::WhenFree => format!("delivered: {label} reads it once it is free"),
            Delivered::OnEnter => {
                format!("waits for approval: {label} reads it once its owner says so")
            }
        })
    }

    /// The other windows as last seen, told to every pane's `tell` (T83), and this window's name kept with its main
    /// pane's session (Q13).
    fn sync_windows(&mut self) {
        let Some(link) = self.link.clone() else {
            return;
        };
        // A name the owner gave this window, coming back with its main pane's session — tried once per name.
        if let Some(kept) = self.list[0].app.session.window.clone()
            && kept != link.name()
            && self.window_tried.as_deref() != Some(kept.as_str())
        {
            let _ = link.rename(&kept);
            self.window_tried = Some(kept);
        }
        // Who asked about a pane that closed hears nothing more.
        let open: Vec<u64> = self.list.iter().map(|pane| pane.id).collect();
        self.notify_back.retain(|(id, _)| open.contains(id));
        let lines: Vec<String> = link
            .seen()
            .into_iter()
            .filter(|card| !card.off)
            .flat_map(|card| card.seats)
            .map(|seat| {
                // Nothing that changes from turn to turn (working or not): the description stays as the cache keeps it.
                format!(
                    "{}: {}, {}, {} mode, folder {}",
                    glut::permissions::printable(&seat.address),
                    seat.role
                        .as_deref()
                        .map_or("all-rounder".to_owned(), glut::permissions::printable),
                    glut::permissions::printable(&seat.model),
                    glut::permissions::printable(&seat.mode),
                    glut::permissions::printable(&seat.project)
                )
            })
            .collect();
        let window = Some(link.name());
        let addresses: Vec<String> = (0..self.list.len()).map(|at| self.address(at)).collect();
        for (pane, address) in self.list.iter_mut().zip(addresses) {
            pane.app
                .parts
                .pane_link
                .set_sessions(window.clone(), lines.clone());
            pane.app.address = Some(address);
        }
    }

    /// A key, a paste or the mouse: the active pane's, but a click moves into the pane under it (the click itself
    /// goes nowhere) and the wheel goes to the pane under it.
    pub fn on_terminal(&mut self, event: TermEvent) {
        // The window came to the front or went (T53): only notifications care.
        if matches!(event, TermEvent::FocusGained | TermEvent::FocusLost) {
            self.notifier.focus(event == TermEvent::FocusGained);
            return;
        }
        // ⌃L draws the screen anew, as in a shell: a terminal that cleared itself (an editor's ⌘K) left it blank.
        if let TermEvent::Key(key) = &event
            && key.kind == crossterm::event::KeyEventKind::Press
            && key.code == crossterm::event::KeyCode::Char('l')
            && key.modifiers == crossterm::event::KeyModifiers::CONTROL
        {
            self.redraw = true;
            return;
        }
        // The arena's card over the screen (T61) takes the keys while it shows; the wheel scrolls its diff.
        if self.card.as_ref().is_some_and(|card| card.drawn) {
            let scrolled =
                |code| crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
            match &event {
                TermEvent::Key(key) => self.on_card_key(*key),
                TermEvent::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        self.on_card_key(scrolled(crossterm::event::KeyCode::Down))
                    }
                    MouseEventKind::ScrollUp => {
                        self.on_card_key(scrolled(crossterm::event::KeyCode::Up))
                    }
                    _ => {}
                },
                _ => {}
            }
            return;
        }
        // A background agent's stream over the pane is only to read: Esc, q (T71) or ⌃C close it, nothing else goes
        // through.
        if self.peek.is_some() {
            if let TermEvent::Key(key) = &event
                && (key.code == crossterm::event::KeyCode::Esc
                    || (key.code == crossterm::event::KeyCode::Char('q')
                        && key.modifiers.is_empty())
                    || (key.code == crossterm::event::KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)))
            {
                self.close_peek();
            }
            // The wheel scrolls the stream read; it answers nothing.
            if let TermEvent::Mouse(mouse) = &event
                && matches!(
                    mouse.kind,
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                )
                && let Some(at) = self.list.iter().position(|pane| Some(pane.id) == self.peek)
            {
                self.list[at].app.on_terminal(event);
            }
            return;
        }
        // A click on a tile opens where it came from (T69); nothing beneath it gets the click.
        if let TermEvent::Mouse(mouse) = &event
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(goes) = self.tiles.clicked(mouse.column, mouse.row)
        {
            self.tile_pressed = true;
            return self.open_tile(goes);
        }
        // The release and the drags of a press on a tile are the tile's.
        if let TermEvent::Mouse(mouse) = &event
            && self.tile_pressed
        {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => return,
                MouseEventKind::Up(MouseButton::Left) => {
                    self.tile_pressed = false;
                    return;
                }
                _ => self.tile_pressed = false,
            }
        }
        // A click on a link opens it (TUI9d): the screen holds the mouse, the terminal never gets to.
        if let TermEvent::Mouse(mouse) = &event
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(url) = crate::links::under(&self.links, mouse.column, mouse.row)
        {
            let said = match crate::links::open(url) {
                Ok(()) => format!("opening {}", crate::links::host_of(url)),
                Err(reason) => reason,
            };
            return self.active().say(said);
        }
        if let TermEvent::Mouse(mouse) = &event
            && self.split()
            && self.on_frame(mouse)
        {
            return;
        }
        if let TermEvent::Mouse(mouse) = &event
            && self.split()
        {
            let under = self
                .list
                .iter()
                .position(|pane| !pane.background && contains(pane.area, mouse.column, mouse.row));
            match (mouse.kind, under) {
                (MouseEventKind::Down(MouseButton::Left), Some(at)) if at != self.active => {
                    return self.focus(at);
                }
                (MouseEventKind::ScrollUp | MouseEventKind::ScrollDown, Some(at)) => {
                    self.list[at].app.on_terminal(event);
                    return;
                }
                _ => {}
            }
        }
        self.active().on_terminal(event);
    }

    /// What the panes asked for since the last look: a split, a close, a toggle.
    pub fn settle(&mut self, host: &Arc<dyn Host>) {
        // A kept layout comes back one pane at a time (TUI9e): the next once the one before stands.
        if self.restoring.is_some() && self.building.is_none() {
            self.restore_step(host);
        }
        self.step_loops(Instant::now());
        self.step_arena(host);
        // Between a bench's tasks there is no arena to step: the next task comes once there is room.
        if self.arena.is_none() {
            self.step_bench();
        }
        // One at a time: a close changes the list the next is looked for in.
        while let Some(at) = self
            .list
            .iter()
            .position(|pane| pane.app.pane_request.is_some())
        {
            let Some(request) = self.list[at].app.pane_request.take() else {
                break;
            };
            // Only a pane on the screen asks the screen for anything; the keys never go behind it.
            if self.list[at].background {
                continue;
            }
            if at != self.active {
                self.focus(at);
            }
            match request {
                Request::Toggle if !self.split() => {
                    self.active().say(tr!(one_pane_split));
                }
                Request::Toggle => {
                    // Round the panes on the screen in reading order; background agents are not on it.
                    let next = (1..self.list.len())
                        .map(|step| (self.active + step) % self.list.len())
                        .find(|at| !self.list[*at].background)
                        .unwrap_or(self.active);
                    self.focus(next);
                }
                Request::Close => self.close(),
                Request::CloseAsked => self.close_asked(),
                Request::ClosePane(id) => self.close_pane(id),
                Request::CloseAs(leaving) => self.leave(leaving),
                Request::Split {
                    resume,
                    place,
                    role,
                } => self.start(resume, place, role, host),
                Request::Send { to, text, whole } => {
                    // A first word naming no pane or agent is the message's own (`/send 3 files changed`).
                    // A window seen (T83) names one too, as the `/send` list puts it there; a mere hyphen does not,
                    // or `/send re-check this` would leave the pane beside.
                    let named = to.as_deref().is_some_and(|word| {
                        self.resolve(word, self.active).is_some()
                            || self
                                .link
                                .as_ref()
                                .is_some_and(|link| link.names_other(word))
                    });
                    if named {
                        self.send(to, Some(text));
                    } else {
                        self.send(None, Some(whole));
                    }
                }
                // "update plan" is a message when "update" names no pane or agent.
                Request::SendPlan { to: Some(word) }
                    if self.resolve(&word, self.active).is_none() =>
                {
                    self.send(None, Some(format!("{word} plan")));
                }
                Request::SendPlan { to } => self.send(to, None),
                Request::SendPick => self.send_picker(),
                Request::Btw {
                    to,
                    question,
                    whole,
                } => {
                    let asked = to
                        .as_deref()
                        .and_then(|word| self.resolve(word, self.active));
                    match asked {
                        Some(at) => {
                            let lent = self.list[at].app.side_asked();
                            let label = self.shown_label(at);
                            self.active().ask_aside(lent, Some(label), question);
                        }
                        None => {
                            let lent = self.active().side_asked();
                            self.active().ask_aside(lent, None, whole);
                        }
                    }
                }
                Request::Background {
                    name,
                    role,
                    place,
                    sure,
                } => {
                    self.start_background(name, role, place, sure, host);
                }
                Request::Swap(with) => self.swap(with),
                Request::Sessions(Some(on)) => {
                    self.sessions_off = !on;
                    self.active().say(if on {
                        tr!(sessions_on)
                    } else {
                        tr!(sessions_off)
                    });
                }
                Request::Sessions(None) => self.sessions_picker(),
                Request::Name(name) => self.name_window(name.trim()),
                Request::Widen(by) => self.widen(by),
                Request::Show(name) => self.show(&name),
                Request::Hide => self.hide(),
                Request::Peek(None) => self.close_peek(),
                Request::End(word) => self.close_named(&word),
                Request::Restore => self.restore(),
                Request::LoopStart(asked) => self.start_loop(asked, host),
                Request::LoopStop(name) => self.stop_loop(name.as_deref()),
                Request::LoopList => self.list_loops(),
                Request::Switch(to) => self.switch(to),
                Request::SwitchNow(to) => self.switch_to = Some(to),
                Request::Arena(asked) => self.start_arena(asked),
                Request::ArenaTake(at, sure) => self.arena_take(at, sure),
                Request::ArenaEnd(sure) => self.end_arena(sure),
                Request::ArenaCard => self.open_card(),
                Request::ArenaBench { set, contenders } => self.start_bench(set, contenders),
                Request::ArenaCheck(command) => self.arena_check(command),
                Request::Peek(Some(name)) => match self.resolve_background(&name) {
                    Some(at) => self.peek = Some(self.list[at].id),
                    _ => self.active().tell(tr!(
                        no_background_peek,
                        name = crate::transcript::clean(&name)
                    )),
                },
            }
        }
    }

    /// Whether the screen ends: a closed window or a kill ends every pane; a `/quit` of any pane ends them all —
    /// said once more first while another works.
    pub fn quitting(&mut self) -> bool {
        // A switch the owner chose (T44) ends the window as a `/quit` does.
        if self.switch_to.is_some() {
            return true;
        }
        if self.list.iter().any(|pane| pane.app.abandoned) {
            self.list.iter_mut().for_each(|pane| pane.app.abandon());
            return true;
        }
        let Some(at) = self.list.iter().position(|pane| pane.app.quit) else {
            return false;
        };
        let working: Vec<String> = (0..self.list.len())
            .filter(|other| *other != at && self.list[*other].app.working())
            .map(|other| self.shown_label(other))
            .collect();
        let armed = self.quit_armed.is_some_and(|since| since.elapsed() < ARMED);
        if working.is_empty() || armed {
            return true;
        }
        self.quit_armed = Some(Instant::now());
        let app = &mut self.list[at].app;
        app.quit = false;
        app.tell(tr!(still_works_quit, who = listed(&working)));
        false
    }

    /// Glut goes on in `folder` (T44): at once while nothing works; else the owner says so first — no the default, no
    /// key typed before the question showed answering it — and every turn and agent stops as at a `/quit`, kept.
    fn switch(&mut self, to: Switch) {
        if to.folder == self.cwd {
            return self.active().tell(tr!(project_same));
        }
        let working: Vec<String> = (0..self.list.len())
            .filter(|at| self.list[*at].app.working())
            .map(|at| self.shown_label(at))
            .collect();
        if working.is_empty() {
            self.switch_to = Some(to);
            return;
        }
        let name = crate::transcript::clean(&to.folder.file_name().map_or_else(
            || to.folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ));
        let items = vec![
            crate::picker::Item::plain(
                tr!(switch_no),
                tr!(switch_no_detail),
                crate::picker::Pick::Shown,
            ),
            crate::picker::Item::plain(
                tr!(switch_yes, name = name),
                tr!(switch_yes_detail),
                crate::picker::Pick::Pane(Request::SwitchNow(to)),
            ),
        ];
        let question = tr!(switch_question, who = listed(&working));
        self.active().picker = Some(crate::picker::Picker::new(question, items, None).guarded());
    }

    /// Closes the active pane; a turn under way is stopped — said once first. The last pane is not closed.
    /// `/close` alone: where more than this pane could close, the list of them to pick from — this one first and
    /// chosen, so ⏎ does what `/close` always did.
    fn close_asked(&mut self) {
        let closable: Vec<usize> = (0..self.list.len())
            .filter(|at| self.list[*at].id != self.grid.main)
            .collect();
        if closable.iter().all(|at| *at == self.active) {
            return self.close();
        }
        self.close_picker(None);
    }

    /// `/close <word>`: the pane of that number, the agent of that name, the pane playing that role — the list to pick
    /// from where the word names no single one.
    fn close_named(&mut self, word: &str) {
        match self.resolve(word, usize::MAX) {
            Some(at) => self.close_pane(self.list[at].id),
            None => self.close_picker(Some(word)),
        }
    }

    /// The panes and agents `/close` can close, the asking pane first; `word`: what named no single one.
    fn close_picker(&mut self, word: Option<&str>) {
        use crate::picker::{Item, Pick, Picker};
        let mut order: Vec<usize> = (0..self.list.len())
            .filter(|at| self.list[*at].id != self.grid.main)
            .collect();
        order.sort_by_key(|at| *at != self.active);
        if order.is_empty() {
            return self.active().tell(tr!(main_pane_stays));
        }
        let items: Vec<Item> = order
            .iter()
            .map(|at| {
                let pane = &self.list[*at];
                let mut detail = format!(
                    "{} · {}",
                    pane.app.role.as_deref().unwrap_or("all-rounder"),
                    crate::transcript::clean(pane.app.engine.model_name())
                );
                if *at == self.active {
                    detail = format!("{} · {detail}", tr!(close_this));
                }
                Item::plain(self.shown_label(*at), detail, Pick::ClosePane(pane.id))
            })
            .collect();
        if let Some(word) = word {
            let said = tr!(close_which, word = crate::transcript::clean(word));
            self.active().say(said);
        }
        self.active().picker = Some(Picker::new(tr!(close_title), items, None));
    }

    /// Closes the pane or agent of session `id` without the keys going there: the owner stays in their pane — unless
    /// the one closing asks what becomes of its worktree, which it asks where it is.
    fn close_pane(&mut self, id: u64) {
        let Some(at) = self.list.iter().position(|pane| pane.id == id) else {
            return;
        };
        if self.list[at].background {
            let name = self.list[at].name.clone().unwrap_or_default();
            return self.end_background(&name);
        }
        if at == self.active {
            return self.close();
        }
        let keys = self.list[self.active].id;
        self.active = at;
        self.return_to = Some(keys);
        self.close();
        self.return_to = None;
        let still = self.list.iter().position(|pane| pane.id == id);
        match still {
            // The worktree's question stands in that pane: the keys stay there to answer it.
            Some(at) if self.list[at].app.picker.is_some() => self.focus(at),
            // It works: the second `/close` closes it — said where the owner is.
            Some(_) => {
                self.focus(self.at(keys));
                self.active().tell(tr!(close_working));
            }
            None => {}
        }
    }

    fn close(&mut self) {
        if !self.split() {
            return self.active().tell(tr!(last_pane_quit));
        }
        if self.list[self.active].id == self.grid.main {
            return self.active().tell(tr!(main_pane_stays));
        }
        // An arena's model's worktree ends with the arena (T61): taken through `/arena take`, removed by `/arena end`.
        let id = self.list[self.active].id;
        if self
            .arena
            .as_ref()
            .is_some_and(|arena| arena.members.iter().any(|member| member.pane == Some(id)))
        {
            return self.active().tell(tr!(arena_member_close));
        }
        let number = self.list[self.active].number;
        let armed = self
            .close_armed
            .is_some_and(|(armed, since)| armed == number && since.elapsed() < ARMED);
        if self.active().working() && !armed {
            self.close_armed = Some((number, Instant::now()));
            return self.active().tell(tr!(close_working));
        }
        self.close_armed = None;
        if let Some(tree) = self.list[self.active].tree.clone() {
            // Git goes over the worktree only once nothing writes there any more.
            if self.active().working() {
                self.active().stop_turn();
                return self.active().tell(tr!(close_stopping));
            }
            let picker = leaving_picker(&tree, crate::worktree::standing(&tree));
            self.active().picker = Some(picker);
            return;
        }
        self.remove_active();
    }

    /// How the active worktree's pane ends, picked: kept, taken over or discarded — the pane stays open where that
    /// does not go through, saying why.
    fn leave(&mut self, leaving: Leaving) {
        // A `/swap` while the question was open may have made this pane the main one.
        if self.list[self.active].id == self.grid.main {
            return self.active().tell(tr!(main_pane_stays));
        }
        let Some(tree) = self.list[self.active].tree.clone() else {
            return self.remove_active();
        };
        if self.active().working() {
            return self.active().tell(tr!(close_works_again));
        }
        let standing = crate::worktree::standing(&tree);
        // What cannot be read is taken as work a discard would lose.
        let loses_work = standing
            .as_ref()
            .map_or(true, |standing| standing.loses_work());
        let done = match leaving {
            Leaving::Keep => Ok(tr!(
                worktree_kept,
                place = tree.place.display(),
                branch = tree.branch
            )),
            Leaving::TakeOver => crate::worktree::take_over(&tree),
            Leaving::Discard if loses_work => {
                let picker = discard_picker(&tree, &standing);
                self.active().picker = Some(picker);
                return;
            }
            Leaving::Discard | Leaving::DiscardSure => crate::worktree::remove(&tree, true)
                .map(|()| tr!(worktree_gone, branch = tree.branch)),
        };
        match done {
            Ok(said) => {
                self.remove_active();
                self.active().tell(said);
            }
            Err(reason) => self.active().transcript.note(reason, Tone::Fault),
        }
    }

    /// Takes the active pane away; a turn under way is stopped.
    fn remove_active(&mut self) {
        let mut pane = self.list.remove(self.active);
        self.grid.remove(pane.id);
        let number = pane.number;
        let stopped = pane.app.working();
        pane.app.stop_turn();
        // The keys go to a pane on the screen, never to an agent in the background.
        let near = self.active.min(self.list.len() - 1);
        let next = self
            .return_to
            .filter(|id| {
                self.list
                    .iter()
                    .any(|pane| pane.id == *id && !pane.background)
            })
            .unwrap_or_else(|| {
                (0..self.list.len())
                    .filter(|at| !self.list[*at].background)
                    .min_by_key(|at| at.abs_diff(near))
                    .map_or(self.list[near].id, |at| self.list[at].id)
            });
        self.reorder();
        self.focus(self.at(next));
        // Alone again: the side panel comes back as the owner keeps it.
        if !self.split() {
            let home = self.active().parts.home.clone();
            self.active().panel = crate::prefs::Prefs::load(&home).panel;
        }
        // A worktree's session is found from its own folder; the leaving said where.
        let kept = match (stopped, &pane.tree) {
            (true, _) => tr!(closed_stopped),
            (false, Some(_)) => tr!(closed_kept),
            (false, None) => tr!(closed_resume),
        };
        self.active()
            .tell(tr!(pane_closed, n = number, kept = kept));
    }

    /// Starts a new pane beside the active one: a session in the same folder, on the active pane's provider and
    /// model, in build — built beside the screen.
    fn start(&mut self, resume: bool, place: Place, role: Option<String>, host: &Arc<dyn Host>) {
        self.start_as(resume, place, role, None, None, host);
    }

    /// [`Self::start`], or — with `background` a name — an agent in the background (T36e), running `looping`
    /// (T60a).
    fn start_as(
        &mut self,
        resume: bool,
        place: Place,
        role: Option<String>,
        background: Option<String>,
        looping: Option<crate::looping::Asked>,
        host: &Arc<dyn Host>,
    ) {
        if !self.may_split {
            return self.active().tell(tr!(host_one_pane));
        }
        if background.is_none() && self.visible() >= MOST {
            return self.active().tell(tr!(most_panes_bg, n = MOST));
        }
        if background.is_none() && !self.room_for_one_more() {
            let said = if self.grid.share > SHARE {
                tr!(no_room_narrower)
            } else {
                tr!(no_room)
            };
            return self.active().tell(said);
        }
        if self.building.is_some() {
            return self.active().tell(tr!(pane_starting));
        }
        // A role named wrong ends the split here, rather than open a pane with every tool.
        if let Some(role) = &role
            && self.active().parts.config.agents.get(role).is_none()
        {
            let known = self.active().parts.config.agents.known();
            return self
                .active()
                .transcript
                .note(tr!(no_role, name = role, known = known), Tone::Fault);
        }
        let number = (1..=u8::MAX)
            .find(|number| self.list.iter().all(|pane| pane.number != *number))
            .unwrap_or(u8::MAX);
        let cwd = self.active().parts.config.cwd.clone();
        let repo = match &place {
            Place::Here | Place::Kept(_) => Ok(None),
            _ => crate::worktree::repo_of(&cwd),
        };
        let tree = match (place, repo) {
            (Place::Kept(tree), _) => Some(tree),
            (Place::Here, _) | (Place::Ask, Ok(None)) => None,
            (Place::Ask, Err(reason)) => {
                self.active()
                    .tell(tr!(no_worktree_offered, reason = reason));
                None
            }
            // A background agent works here unless told `tree`: it has no pane to ask in.
            (Place::Ask, Ok(Some(_))) if background.is_some() => None,
            (Place::Ask, Ok(Some(repo))) => {
                let picker = where_picker(resume, role.as_deref(), &repo, number);
                self.active().picker = Some(picker);
                return;
            }
            (Place::Tree(_), Err(reason)) => {
                return self.active().transcript.note(reason, Tone::Fault);
            }
            (Place::Tree(_), Ok(None)) => {
                return self.active().tell(tr!(no_repository));
            }
            (Place::Tree(branch), Ok(Some(repo))) => {
                let branch = if branch.is_empty() {
                    crate::worktree::proposed_branch(&repo, number)
                } else {
                    branch
                };
                let home = self.active().parts.home.clone();
                match crate::worktree::add(&home, &repo, &branch) {
                    Ok(tree) => Some(tree),
                    Err(reason) => return self.active().transcript.note(reason, Tone::Fault),
                }
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        let servers = self.servers.clone();
        let app = self.active();
        let mut launch = launch_beside(app, servers);
        if let Some(tree) = &tree {
            launch.cwd = tree.place.clone();
        }
        // A loop does what its pane could, never more (T60a): the pane's mode, the "always" said there, its folders.
        let looping = looping.map(|asked| {
            launch.gate.mode = Some(app.mode());
            launch.gate.allow = app.sender().granted;
            (asked, app.parts.tools.folders().list())
        });
        app.tell(format!("pane {number} starts…"));
        let (reports, inlet) = mpsc::unbounded_channel();
        forward(id, inlet, self.reports.clone());
        self.building = Some(Building {
            id,
            number,
            resume,
            tree,
            role,
            background,
            restore: self.restore_next.take(),
            reports: reports.clone(),
            looping,
            arena: None,
        });
        let built = self.built.clone();
        let host = Arc::clone(host);
        tokio::spawn(async move {
            let _ = built.send(session::build(launch, host, reports).await);
        });
    }

    /// The new pane's session, built — or why not. It takes the keys unless the owner is busy in the active pane
    /// (typing, a question, a list open): a key meant there must not land in the new one.
    pub fn on_built(&mut self, result: Built) {
        // Servers starting may have written to the terminal: the next frame starts from blank.
        self.redraw = true;
        let Some(building) = self.building.take() else {
            return;
        };
        let (parts, engine) = match result {
            Ok(built) => built,
            Err(reason) => {
                if let Some(restoring) = &mut self.restoring {
                    restoring.building = None;
                }
                // A worktree made for it holds nothing yet: it goes with the pane that never came. One found again
                // for a kept pane (TUI9e) holds its work: it stays.
                let gone = match &building.tree {
                    Some(tree) if building.restore.is_some() => {
                        Some(tr!(worktree_stays, place = tree.place.display()))
                    }
                    Some(tree) => match crate::worktree::remove(tree, true) {
                        Ok(()) => Some(tr!(worktree_gone_again, branch = tree.branch)),
                        Err(left) => Some(tr!(
                            worktree_stays_why,
                            place = tree.place.display(),
                            reason = left
                        )),
                    },
                    None => None,
                };
                if let Some(member) = building
                    .arena
                    .and_then(|at| self.arena.as_mut()?.members.get_mut(at))
                {
                    member.state = crate::arena::State::Aborted(crate::transcript::clean(&reason));
                    member.tree = None;
                }
                let failed = tr!(pane_not_started, n = building.number, reason = reason);
                let said = match gone {
                    Some(gone) => tr!(clause_join, first = failed, second = gone),
                    None => failed,
                };
                return self.active().transcript.note(said, Tone::Fault);
            }
        };
        // An arena ended while its model's pane was built (T61): the pane does not come, its worktree goes. Nor does
        // one whose settings, read anew for it, run its commands outside a sandbox in `auto`.
        let unguarded = building.arena.is_some()
            && !session::sandboxed(&parts)
            && parts
                .tools
                .gate()
                .is_some_and(|gate| gate.mode() == glut::permissions::Mode::Auto);
        if unguarded
            && let Some(member) = building
                .arena
                .and_then(|at| self.arena.as_mut()?.members.get_mut(at))
        {
            member.state = crate::arena::State::Aborted(tr!(arena_no_sandbox).into());
            member.tree = None;
        }
        if building.arena.is_some() && (self.arena.is_none() || unguarded) {
            if let Some(tree) = &building.tree {
                let _ = crate::worktree::remove(tree, true);
            }
            return;
        }
        // Nobody watches an arena's model (T61): the Keychain the owner keeps open for sessions stays shut to it, and
        // its gate asks no one — what it would ask is refused, so no model waits on the owner while another works on.
        // Its commands write its copy alone and read nothing of the owner's repository: the copy holds what it needs.
        if building.arena.is_some() {
            parts.tools.keychain().set_session_open(false);
            let tree = building.tree.as_ref();
            // A linked worktree's repository holds the main checkout beside its git folder: hidden too.
            let hidden = tree.map_or_else(Vec::new, |tree| {
                let mut hidden = vec![tree.repo.top.clone(), tree.repo.common.clone()];
                if let Some(main) = tree
                    .repo
                    .common
                    .parent()
                    .filter(|_| tree.repo.common.ends_with(".git"))
                {
                    hidden.push(main.to_path_buf());
                }
                hidden
            });
            parts
                .tools
                .contain(tree.map(crate::worktree::Tree::private_home), hidden);
        }
        let mut app = App::new(Arc::new(parts), engine, building.reports);
        // A worktree's pane belongs to the window's project (T44): `/projects` there marks and leaves that one.
        app.project = self.cwd.clone();
        if building.resume {
            app.picker = Some(app.session_picker());
        }
        if let Some(id) = &building.restore {
            open_kept(&mut app, id);
        }
        if let Some(restoring) = &mut self.restoring
            && let Some(at) = restoring.building.take()
        {
            restoring.ids[at] = Some(building.id);
        }
        if let Err(reason) = app.set_role(building.role.as_deref()) {
            app.transcript.note(reason, Tone::Fault);
        }
        // A background agent stands in no split: it waits for its first message, reached by its name.
        if let Some(name) = building.background {
            app.panel = Some(false);
            let looping = building.looping.map(|(asked, folders)| {
                // Checked when the owner gave them; the pane's tools check each again as they reach it.
                for folder in folders {
                    let _ = app.parts.tools.folders().add(folder);
                }
                let limits = app.loop_limits();
                crate::looping::Loop::new(asked, limits, Arc::clone(&app.parts.loop_link))
            });
            // Where the owner reads the loop's rounds, how it ends: /loop stop, x on its card, Esc in a round.
            if looping.is_some() {
                app.tell(tr!(loop_pane_hint, name = name));
            }
            let said = match &looping {
                Some(looping) => tr!(
                    loop_starts,
                    name = name,
                    every = crate::looping::shown(looping.asked.every),
                    task =
                        crate::transcript::clean(&crate::transcript::cut(&looping.asked.task, 80))
                ),
                None => tr!(background_waits, name = name),
            };
            // On a subscription its tokens end it; without prices only the rounds do: said, not found out.
            let said = match (&looping, app.engine.plan) {
                (Some(looping), Some(known)) => {
                    let said = tr!(
                        clause_join,
                        first = said,
                        second = tr!(
                            loop_on_plan,
                            name = known.name,
                            max = crate::looping::tokens_shown(
                                looping.limits.max_tokens.unwrap_or_default()
                            )
                        )
                    );
                    // Its terms allow interactive use only: the owner's decision, said (PV4 Q8).
                    if known.plan == glut::providers::catalog::Plan::InteractiveOnly {
                        tr!(
                            clause_join,
                            first = said,
                            second = tr!(plan_interactive_only, name = known.name)
                        )
                    } else {
                        said
                    }
                }
                (Some(_), None) if app.engine.prices.is_none() => {
                    tr!(clause_join, first = said, second = tr!(loop_uncounted))
                }
                _ => said,
            };
            self.list.push(Pane {
                looping,
                id: building.id,
                number: building.number,
                name: Some(name.clone()),
                background: true,
                app,
                area: Rect::default(),
                tree: building.tree,
            });
            self.reorder();
            return self.active().tell(said);
        }
        // Panes leave little room beside them: the side panel starts hidden (⌃B brings it, where there is room).
        app.panel = Some(false);
        let current = &mut self.list[self.active].app;
        current.panel = Some(false);
        // An arena's panes come one after another: the keys stay with the owner meanwhile (T61).
        let busy_here = building.arena.is_some()
            || !current.input.is_empty()
            || current.waits_for_owner()
            || current.picker.is_some();
        if let Some(member) = building
            .arena
            .and_then(|at| self.arena.as_mut()?.members.get_mut(at))
        {
            member.state = crate::arena::State::Ready;
            member.pane = Some(building.id);
        }
        let active_id = self.list[self.active].id;
        self.grid.others.push(building.id);
        self.list.push(Pane {
            looping: None,
            id: building.id,
            number: building.number,
            name: None,
            background: false,
            app,
            area: Rect::default(),
            tree: building.tree,
        });
        self.reorder();
        let at = self.at(building.id);
        if busy_here {
            // The keys stay where they were, wherever that pane stands now.
            self.active = self.at(active_id);
            let number = building.number;
            self.active().tell(tr!(pane_ready, n = number));
        } else {
            self.focus(at);
        }
    }

    /// Sends the active pane's `text` — or its plan, with `None` — to `to` (a pane, an agent; the only other one when
    /// none): taken there as if the owner typed it, the keys staying here.
    fn send(&mut self, to: Option<String>, text: Option<String>) {
        let from = self.list[self.active].number;
        let target = match &to {
            Some(word) => self.resolve(word, self.active),
            None => self.only_other(self.active),
        };
        // Not a pane here, but another window (T83): the owner's word goes there.
        if target.is_none()
            && let (Some(word), Some(text)) = (&to, &text)
            && self.reaches_window(word)
        {
            return self.send_window(word.clone(), text.clone());
        }
        // `/send <window>` with nothing after it: the address is no message for the pane beside.
        if to.is_none()
            && let Some(text) = &text
            && self
                .link
                .as_ref()
                .is_some_and(|link| link.names_other(text.trim()))
        {
            return self.active().tell(tr!(send_which));
        }
        let Some(target) = target else {
            let said = match (&to, self.list.len()) {
                (_, 1) => tr!(one_pane_send).to_owned(),
                (Some(word), _) => tr!(no_pane_or_agent, name = crate::transcript::clean(word)),
                (None, _) => tr!(send_which).to_owned(),
            };
            return self.active().tell(said);
        };
        let number = self.list[target].number;
        let label = self.shown_label(target);
        let plan = text.is_none();
        let text = match text {
            Some(text) => text,
            None => match self.active().plan_to_build() {
                Some(plan) => plan,
                None => return self.active().tell(tr!(no_plan_send)),
            },
        };
        // A plan was written by a model: the owner hands it over, the words stay the planner's.
        let message = if plan {
            format!(
                "From pane {from} — its plan, handed over by the owner to be built here:\n\n{text}"
            )
        } else {
            format!("From pane {from}: {text}")
        };
        let delivered = self.list[target].app.deliver(message);
        // Marked handed only once it went.
        if plan && let Some(plan) = &mut self.active().plan {
            plan.state = crate::plan::State::Handed(number);
        }
        use crate::app::Delivered;
        let when = match delivered {
            Delivered::Now => tr!(sent_now),
            Delivered::AfterTurn => tr!(sent_after_turn),
            Delivered::WhenFree => tr!(sent_when_free),
            Delivered::OnEnter => tr!(sent_on_enter),
        };
        self.active().tell(tr!(sent_to, label = label, when = when));
    }

    /// `/arena <models>: <task>` (T61): the arena set up in a git repository with room for its panes; its models'
    /// worktrees and panes then come one after another ([`Self::step_arena`]).
    fn start_arena(&mut self, asked: crate::arena::Asked) {
        if !self.may_split {
            return self.active().tell(tr!(host_one_pane));
        }
        if let Some(arena) = &self.arena {
            let said = if arena.over() {
                tr!(arena_end_first)
            } else {
                tr!(arena_running)
            };
            return self.active().tell(said);
        }
        // A pane of an arena ended meanwhile still builds: its place among the members would be the new one's.
        if self
            .building
            .as_ref()
            .is_some_and(|building| building.arena.is_some())
        {
            return self.active().tell(tr!(pane_starting));
        }
        if self.visible() + asked.contenders.len() > MOST {
            return self
                .active()
                .tell(tr!(arena_no_room, n = asked.contenders.len(), most = MOST));
        }
        let cwd = self.active().parts.config.cwd.clone();
        let repo = match crate::worktree::repo_of(&cwd) {
            Ok(Some(repo)) => repo,
            Ok(None) => return self.active().tell(tr!(arena_no_repository)),
            Err(reason) => return self.active().transcript.note(reason, Tone::Fault),
        };
        let home = self.list[self.active].id;
        if let Err(reason) = self.begin_arena(asked, repo, home, None) {
            self.active().transcript.note(reason, Tone::Fault);
        }
    }

    /// An arena of `asked` on `repo`, with a bench task's limits (else the free arena's): the room for its panes
    /// checked, the owner's `HEAD` written down, what it runs said.
    fn begin_arena(
        &mut self,
        asked: crate::arena::Asked,
        repo: crate::worktree::Repo,
        home: u64,
        limits: Option<(Duration, f64)>,
    ) -> Result<(), String> {
        if self.arena.is_some() {
            return Err(tr!(arena_end_first).into());
        }
        if self
            .building
            .as_ref()
            .is_some_and(|building| building.arena.is_some())
        {
            return Err(tr!(pane_starting).into());
        }
        if self.visible() + asked.contenders.len() > MOST {
            return Err(tr!(arena_no_room, n = asked.contenders.len(), most = MOST));
        }
        let head = crate::worktree::head(&repo)?;
        let mut arena = crate::arena::Arena::new(asked, home);
        arena.repo = Some(repo);
        arena.head = head;
        if let Some((time, cost)) = limits {
            arena.time_limit = time;
            arena.cost_limit = cost;
        }
        let names: Vec<String> = arena
            .members
            .iter()
            .map(|member| crate::transcript::clean(&member.contender.named()))
            .collect();
        let local = arena.members.iter().any(|member| member.contender.local);
        let said = tr!(
            arena_starts,
            models = names.join(", "),
            minutes = arena.time_limit.as_secs() / 60,
            dollars = format!("{:.2}", arena.cost_limit)
        );
        let said = if local {
            tr!(clause_join, first = said, second = tr!(arena_local_later))
        } else {
            said
        };
        self.note_in(home, said, Tone::Quiet);
        self.arena = Some(arena);
        Ok(())
    }

    /// `/arena tasks <set>` (step 2): the set's tasks one after another on `contenders`, each an arena of its own on
    /// a fresh repository of the task's start, judged by the task's check — only where panes may split and a sandbox
    /// holds the models, as nobody watches them.
    fn start_bench(&mut self, set: String, contenders: Vec<crate::arena::Contender>) {
        if !self.may_split {
            return self.active().tell(tr!(host_one_pane));
        }
        if !session::sandboxed(&self.active().parts) {
            return self.active().tell(tr!(arena_no_sandbox));
        }
        if self.arena.is_some() || self.bench.is_some() {
            return self.active().tell(tr!(arena_end_first));
        }
        let home = self.active().parts.home.clone();
        let tasks = match crate::bench::tasks(&crate::bench::root(&home), &set) {
            Ok(tasks) => tasks,
            Err(reason) => return self.active().transcript.note(reason, Tone::Fault),
        };
        let names: Vec<String> = contenders
            .iter()
            .map(|contender| crate::transcript::clean(&contender.named()))
            .collect();
        self.active().tell(tr!(
            bench_starts,
            set = set.clone(),
            n = tasks.len(),
            models = names.join(", ")
        ));
        self.bench = Some(crate::bench::Bench {
            home,
            home_pane: self.list[self.active].id,
            set,
            contenders,
            total: tasks.len(),
            left: tasks.into(),
            current: None,
            kept: Vec::new(),
        });
        self.next_task();
    }

    /// The bench's next task as an arena, once nothing of the last one stands and there is room for its panes (else
    /// it waits for a later turn of the loop); once none is left, the set's results.
    fn next_task(&mut self) {
        loop {
            let Some(bench) = &self.bench else {
                return;
            };
            if self.arena.is_some()
                || self.building.is_some()
                || bench.current.is_some()
                || self.visible() + bench.contenders.len() > MOST
            {
                return;
            }
            let (set, total) = (bench.set.clone(), bench.total);
            let (home_pane, contenders) = (bench.home_pane, bench.contenders.clone());
            let Some(task) = self.bench.as_mut().and_then(|bench| bench.left.pop_front()) else {
                // This run's results alone: an earlier run of the set is `/arena results`' to show.
                let kept = self
                    .bench
                    .take()
                    .map(|bench| bench.kept)
                    .unwrap_or_default();
                let said = crate::bench::finished(&set, &kept);
                return self.note_in(home_pane, said, Tone::Fine);
            };
            let number = total - self.bench.as_ref().map_or(0, |bench| bench.left.len());
            let started = crate::bench::prepare(&set, &task).and_then(|place| {
                let asked = crate::arena::Asked {
                    contenders,
                    task: task.text.clone(),
                };
                let begun = match crate::worktree::repo_of(&place) {
                    Ok(Some(repo)) => self.begin_arena(
                        asked,
                        repo,
                        home_pane,
                        Some((task.time_limit, task.cost_limit)),
                    ),
                    Ok(None) => Err(tr!(arena_no_repository).to_owned()),
                    Err(reason) => Err(reason),
                };
                if begun.is_err() {
                    crate::bench::forget(&place);
                }
                begun.map(|()| place)
            });
            match started {
                Ok(place) => {
                    let said = tr!(
                        bench_task,
                        n = number,
                        total = total,
                        task = task.name.clone()
                    );
                    self.note_in(home_pane, said, Tone::Quiet);
                    if let Some(bench) = &mut self.bench {
                        bench.current = Some((task, place));
                    }
                    return;
                }
                // A task that cannot start is passed by no model: said, and the next one comes.
                Err(reason) => {
                    let said = tr!(
                        bench_task_failed,
                        task = task.name.clone(),
                        reason = crate::transcript::clean(&reason)
                    );
                    self.note_in(home_pane, said, Tone::Fault);
                }
            }
        }
    }

    /// A note in pane `id`, else in the active one.
    fn note_in(&mut self, id: u64, said: String, tone: Tone) {
        match self.list.iter().position(|pane| pane.id == id) {
            Some(at) => self.list[at].app.transcript.note(said, tone),
            None => self.active().transcript.note(said, tone),
        }
    }

    /// A bench task's check for every model that answered: in a copy Glut makes in `~/.glut` of the task's start
    /// and the model's checked patch, with the task's own check files ([`crate::worktree::check_copy`]), run there in
    /// a sandboxed shell of its own ([`App::arena_check_in`]). A patch the copy refuses fails the check, its reason
    /// said in that pane.
    fn bench_checks(&mut self) {
        let Some(folder) = self
            .bench
            .as_ref()
            .and_then(|bench| bench.current.as_ref())
            .map(|(task, _)| task.folder.clone())
        else {
            return;
        };
        let Some(home) = self.bench.as_ref().map(|bench| bench.home.clone()) else {
            return;
        };
        let Some(mut arena) = self.arena.take() else {
            return;
        };
        arena.checking = true;
        for (at, member) in arena.members.iter().enumerate() {
            if member.state != crate::arena::State::Done(glut::FinishReason::Answered) {
                continue;
            }
            let (Some(tree), Some(start), Some(now)) =
                (&member.tree, &member.start, &member.snapshot)
            else {
                continue;
            };
            let Some(pane) = self
                .list
                .iter_mut()
                .find(|pane| Some(pane.id) == member.pane)
            else {
                continue;
            };
            let place = check_place(&home, &arena, at);
            match crate::worktree::check_copy(tree, start, now, &folder, &place) {
                Ok(()) => {
                    let started = pane.app.arena_check_in(place, "sh .arena/check.sh");
                    if started.is_some() {
                        arena.checks[at] = crate::arena::Check::Running;
                    }
                    arena.check_ids[at] = started;
                }
                Err(reason) => {
                    pane.app.transcript.note(
                        tr!(
                            bench_check_refused,
                            reason = crate::transcript::clean(&reason)
                        ),
                        Tone::Fault,
                    );
                    arena.check_said[at] = Some(crate::transcript::clean(&reason));
                    arena.checks[at] = crate::arena::Check::Failed;
                }
            }
        }
        self.arena = Some(arena);
    }

    /// Once a bench task's checks are over: the arena ended without asking (its worktrees are Glut's own, of a
    /// repository made for the task); once it is gone, every model's result kept and the next task begun. A check
    /// that never ran (no answer, a pane closed) is said so in the result, not counted as a model's failure alone.
    fn step_bench(&mut self) {
        let Some(bench) = &self.bench else {
            return;
        };
        let Some((task, place)) = bench.current.clone() else {
            // Between tasks: the next one, once there is room.
            return self.next_task();
        };
        let (home, set, home_pane) = (bench.home.clone(), bench.set.clone(), bench.home_pane);
        let Some(arena) = &self.arena else {
            return;
        };
        if !arena.checking || arena.checks.contains(&crate::arena::Check::Running) {
            return;
        }
        let rows = self.rows(arena);
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let records: Vec<crate::bench::Record> = arena
            .members
            .iter()
            .zip(&rows)
            .enumerate()
            .map(|(at, (member, row))| {
                let check = arena.checks[at];
                let ended = if !row.answered {
                    Some(row.state.clone())
                } else if check == crate::arena::Check::None {
                    Some(tr!(bench_unchecked).to_owned())
                } else if check == crate::arena::Check::Failed {
                    arena.check_said[at].clone()
                } else {
                    None
                };
                crate::bench::Record {
                    date: date.clone(),
                    set: set.clone(),
                    task: task.name.clone(),
                    model: member.contender.named(),
                    passed: row.answered && check == crate::arena::Check::Passed,
                    seconds: row.seconds,
                    dollars: row.dollars,
                    tokens: row.token_count,
                    refusals: member.refusals,
                    ended,
                }
            })
            .collect();
        let said = crate::arena::compared(arena, &rows, Some(&arena.checks));
        let arena_now = arena.clone();
        // The task counts once its arena is gone: taken out of the bench first, as `end_arena` reads an owner's end
        // there; put back when something still holds the arena, for the next turn of the loop to try again.
        let current = self.bench.as_mut().and_then(|bench| bench.current.take());
        self.end_arena(true);
        if self.arena.is_some() {
            if let Some(bench) = &mut self.bench {
                bench.current = current;
            }
            return;
        }
        if let Err(reason) = crate::bench::keep(&home, &records) {
            self.note_in(home_pane, reason, Tone::Fault);
        }
        if let Some(bench) = &mut self.bench {
            bench.kept.extend(records.iter().cloned());
        }
        for at in 0..records.len() {
            let _ = std::fs::remove_dir_all(check_place(&home, &arena_now, at));
        }
        self.note_in(home_pane, said, Tone::Fine);
        crate::bench::forget(&place);
        self.next_task();
    }

    /// The arena's next steps, at every turn of the screen's loop: a model past a limit stopped, the task given to
    /// each model whose turn it is, the next model's worktree and pane made, and once all are over their numbers
    /// shown in the pane it started from.
    fn step_arena(&mut self, host: &Arc<dyn Host>) {
        let Some(mut arena) = self.arena.take() else {
            return;
        };
        let limits = (arena.time_limit, arena.cost_limit);
        for member in &mut arena.members {
            let crate::arena::State::Working = member.state else {
                continue;
            };
            let Some(at) = self
                .list
                .iter()
                .position(|pane| Some(pane.id) == member.pane)
            else {
                member.state = crate::arena::State::Aborted(tr!(arena_closed).into());
                continue;
            };
            let began = member.began.unwrap_or_else(Instant::now);
            if let Some(why) =
                crate::arena::past_limit(limits.0, limits.1, began, self.list[at].app.cost)
            {
                self.list[at].app.stop_turn();
                self.list[at]
                    .app
                    .tell(tr!(arena_stopped, why = why.clone()));
                member.took = Some(began.elapsed());
                member.state = crate::arena::State::Aborted(why);
            }
        }
        // A check whose pane closed never reports: it is no longer running.
        for (at, member) in arena.members.iter().enumerate() {
            if arena.checks[at] == crate::arena::Check::Running
                && !self.list.iter().any(|pane| Some(pane.id) == member.pane)
            {
                arena.checks[at] = crate::arena::Check::None;
            }
        }
        while let Some(due) = arena.next_due() {
            let member = &mut arena.members[due];
            match self
                .list
                .iter()
                .position(|pane| Some(pane.id) == member.pane)
            {
                // A pane busy otherwise (a list open, a message waiting) gets it once free: its clock starts then.
                Some(at) if !self.list[at].app.takes_now() => break,
                Some(at) => {
                    self.list[at].app.deliver(arena.task.clone());
                    member.state = crate::arena::State::Working;
                    member.began = Some(Instant::now());
                }
                None => member.state = crate::arena::State::Aborted(tr!(arena_closed).into()),
            }
        }
        if self.building.is_none()
            && let Some(next) = arena
                .members
                .iter()
                .position(|member| member.state == crate::arena::State::Waiting)
        {
            // A model without room waits for it, never counts as failed: a wider window or a closed pane starts it.
            if !self.room_for_one_more() {
                if !arena.told_no_room {
                    arena.told_no_room = true;
                    let model = crate::transcript::clean(&arena.members[next].contender.named());
                    self.note_in(
                        arena.home,
                        tr!(arena_waits_for_room, model = model),
                        Tone::Quiet,
                    );
                }
            } else {
                arena.told_no_room = false;
                if let Err(reason) = self.start_member(&mut arena, next, host) {
                    arena.members[next].state =
                        crate::arena::State::Aborted(crate::transcript::clean(&reason));
                }
            }
        }
        // Compared once every model is over and no stopped turn still writes in its worktree.
        let resting = arena.members.iter().all(|member| {
            self.list
                .iter()
                .find(|pane| Some(pane.id) == member.pane)
                .is_none_or(|pane| !pane.app.working())
        });
        let compared_now = arena.over() && !arena.compared && resting;
        if compared_now {
            arena.compared = true;
            snapshots(&mut arena);
            // A bench's task is told once, with its checks, when they are done (`step_bench`).
            if self.bench.is_none() {
                let said = crate::arena::compared(&arena, &self.rows(&arena), None);
                match self.list.iter().position(|pane| pane.id == arena.home) {
                    Some(at) => self.list[at].app.transcript.note(said, Tone::Fine),
                    None => self.active().transcript.note(said, Tone::Fine),
                }
            }
        }
        self.arena = Some(arena);
        // Once all are over the check kept for this repository runs in every worktree; the card waits for the owner's
        // `/arena card`, never opening under a key typed for something else.
        if compared_now && self.bench.is_some() {
            self.bench_checks();
        } else if compared_now && self.check_command().is_some() {
            self.arena_check(None);
        }
        self.step_bench();
    }

    /// Member `at`'s worktree made from the owner's folder as it is, and its pane started there on its model, in
    /// `auto` where a sandbox holds its commands (else in build, which asks before every command).
    fn start_member(
        &mut self,
        arena: &mut crate::arena::Arena,
        at: usize,
        host: &Arc<dyn Host>,
    ) -> Result<(), String> {
        if !self.room_for_one_more() {
            return Err(tr!(no_room).into());
        }
        let Some(repo) = arena.repo.clone() else {
            return Err(tr!(arena_no_repository).into());
        };
        let from = self
            .list
            .iter()
            .position(|pane| pane.id == arena.home)
            .unwrap_or(self.active);
        let home = self.list[from].app.parts.home.clone();
        let tree = crate::worktree::clone_for_arena(&home, &repo, &arena.branch(at))?;
        let mirrored = match crate::worktree::mirror(&tree) {
            Ok(mirrored) => mirrored,
            Err(reason) => {
                let _ = crate::worktree::remove(&tree, true);
                return Err(reason);
            }
        };
        if !mirrored.left_out.is_empty() {
            let shown: Vec<&str> = mirrored
                .left_out
                .iter()
                .take(4)
                .map(String::as_str)
                .collect();
            self.list[from]
                .app
                .tell(tr!(arena_left_out, n = at + 1, files = shown.join(", ")));
        }
        let member = &mut arena.members[at];
        member.tree = Some(tree.clone());
        member.start = Some(mirrored.start);
        member.state = crate::arena::State::Building;
        let number = (1..=u8::MAX)
            .find(|number| self.list.iter().all(|pane| pane.number != *number))
            .unwrap_or(u8::MAX);
        let id = self.next_id;
        self.next_id += 1;
        let sandboxed = session::sandboxed(&self.list[from].app.parts);
        // Unattended: none of the owner's servers, which run outside the sandbox, and no message to another pane.
        let mut launch = launch_beside(&self.list[from].app, crate::host::Servers::None);
        launch.gate.deny.push("tell".to_owned());
        launch.cwd = tree.place.clone();
        launch.engine = crate::host::EngineChoice::Providers {
            provider: Some(member.contender.provider.clone()),
            model: Some(member.contender.model.clone()),
        };
        launch.gate.mode = Some(if sandboxed {
            glut::permissions::Mode::Auto
        } else {
            glut::permissions::Mode::AcceptEdits
        });
        let (reports, inlet) = mpsc::unbounded_channel();
        forward(id, inlet, self.reports.clone());
        self.building = Some(Building {
            id,
            number,
            resume: false,
            tree: Some(tree),
            role: None,
            background: None,
            restore: None,
            looping: None,
            arena: Some(at),
            reports: reports.clone(),
        });
        let built = self.built.clone();
        let host = Arc::clone(host);
        tokio::spawn(async move {
            let _ = built.send(session::build(launch, host, reports).await);
        });
        Ok(())
    }

    /// Each arena model's numbers: what its pane spent, what its worktree changed.
    fn rows(&self, arena: &crate::arena::Arena) -> Vec<crate::arena::Row> {
        let spent: Vec<crate::arena::Spent> = arena
            .members
            .iter()
            .map(|member| {
                let Some(pane) = self.list.iter().find(|pane| Some(pane.id) == member.pane) else {
                    return crate::arena::Spent::default();
                };
                let app = &pane.app;
                crate::arena::Spent {
                    cost: app.cost,
                    tokens: app.usage.input_tokens + app.usage.output_tokens,
                    calls: app.meter.turns.iter().map(|turn| turn.calls).sum(),
                    tools: app.meter.turns.iter().map(|turn| turn.tools).sum(),
                }
            })
            .collect();
        let changes: Vec<Result<crate::worktree::Changes, String>> = arena
            .members
            .iter()
            .map(
                |member| match (&member.tree, &member.start, &member.snapshot) {
                    (Some(tree), Some(start), Some(now)) => {
                        crate::worktree::changes(tree, start, now)
                    }
                    _ => Err(tr!(arena_no_tree).into()),
                },
            )
            .collect();
        crate::arena::rows(arena, &spent, &changes)
    }

    /// `/arena card`: the comparison card over the screen, read from each worktree as it stood once every model was
    /// over.
    fn open_card(&mut self) {
        let Some(arena) = &self.arena else {
            return self.active().tell(tr!(arena_none));
        };
        if !arena.compared {
            return self.active().tell(tr!(arena_running));
        }
        let rows = self.rows(arena);
        let diffs = arena
            .members
            .iter()
            .map(
                |member| match (&member.tree, &member.start, &member.snapshot) {
                    (Some(tree), Some(start), Some(now)) => {
                        crate::worktree::diff_text(tree, start, now)
                    }
                    _ => Err(tr!(arena_no_tree).into()),
                },
            )
            .collect();
        let selected = self.card.as_ref().map_or(0, |card| card.selected);
        self.card = Some(crate::arena::Card::new(rows, diffs, selected));
    }

    /// The check command kept for the arena's repository.
    fn check_command(&self) -> Option<String> {
        let repo = self.arena.as_ref()?.repo.as_ref()?;
        let home = &self.list[self.active].app.parts.home;
        crate::prefs::Prefs::load(home)
            .arena_checks
            .get(&repo.top.to_string_lossy().into_owned())
            .cloned()
    }

    /// `/arena check [command]` (and `c` on the card): `command` kept for the arena's repository, then the kept one
    /// run in every model's worktree, in its pane's sandbox — once every model is over.
    fn arena_check(&mut self, command: Option<String>) {
        // A bench task's own check decides its result.
        if self.bench.is_some() {
            return self.active().tell(tr!(bench_no_check));
        }
        let Some(arena) = &self.arena else {
            return self.active().tell(tr!(arena_none));
        };
        if let Some(command) = command
            && let Some(repo) = &arena.repo
        {
            let top = repo.top.to_string_lossy().into_owned();
            let home = self.list[self.active].app.parts.home.clone();
            let mut prefs = crate::prefs::Prefs::load(&home);
            prefs.arena_checks.insert(top, command.clone());
            if let Err(reason) = prefs.save(&home) {
                self.active().transcript.note(reason, Tone::Fault);
            }
            self.active().tell(tr!(
                arena_check_kept,
                command = crate::transcript::clean(&command)
            ));
        }
        let Some(command) = self.check_command() else {
            return self.active().tell(tr!(arena_check_usage));
        };
        let Some(mut arena) = self.arena.take() else {
            return;
        };
        if arena.compared {
            for (at, member) in arena.members.iter().enumerate() {
                // One check at a time in a worktree, and none where a stopped turn still writes.
                if arena.checks[at] == crate::arena::Check::Running {
                    continue;
                }
                let pane = self
                    .list
                    .iter_mut()
                    .find(|pane| Some(pane.id) == member.pane)
                    .filter(|pane| !pane.app.working());
                if let (Some(pane), Some(_)) = (pane, &member.tree) {
                    let started = pane.app.arena_check(&command);
                    arena.checks[at] = if started.is_some() {
                        crate::arena::Check::Running
                    } else {
                        crate::arena::Check::None
                    };
                    arena.check_ids[at] = started;
                }
            }
        } else {
            self.active().tell(tr!(arena_running));
        }
        self.arena = Some(arena);
    }

    /// A key on the comparison card: its own, or what it asks of the screen.
    fn on_card_key(&mut self, key: crossterm::event::KeyEvent) {
        let Some(card) = &mut self.card else {
            return;
        };
        match card.key(key) {
            crate::arena::CardAction::None => {}
            crate::arena::CardAction::Close => self.card = None,
            crate::arena::CardAction::Check => self.arena_check(None),
            crate::arena::CardAction::Take(at) => {
                if let Some(home) = self.arena.as_ref().map(|arena| arena.home)
                    && let Some(at) = self.list.iter().position(|pane| pane.id == home)
                {
                    self.focus(at);
                }
                self.arena_take(at, false);
                // The card makes room for the question; a refusal is said in the pane, the card stays.
                if self.active().picker.is_some() {
                    self.card = None;
                }
            }
        }
    }

    /// `/arena take <n>`: model n's changes into the owner's folder, once every model is over and no other's were
    /// taken — first its files shown and asked, then taken (`sure`).
    fn arena_take(&mut self, at: usize, sure: bool) {
        // A bench task's changes go into its own start repository, never the owner's folder: nothing to take.
        if self.bench.is_some() {
            return self.active().tell(tr!(bench_no_take));
        }
        let Some(arena) = &self.arena else {
            return self.active().tell(tr!(arena_none));
        };
        if !arena.over() {
            return self.active().tell(tr!(arena_running));
        }
        if let Some(taken) = arena.taken {
            return self.active().tell(tr!(arena_taken_already, n = taken + 1));
        }
        let Some(member) = arena.members.get(at) else {
            return self.active().tell(tr!(arena_no_member, n = at + 1));
        };
        let (Some(tree), Some(start)) = (member.tree.clone(), member.start.clone()) else {
            return self.active().tell(tr!(arena_no_tree));
        };
        // A stopped turn may still be ending: nothing is taken while something writes there.
        if let Some(pane) = self.list.iter().find(|pane| Some(pane.id) == member.pane)
            && pane.app.working()
        {
            return self.active().tell(tr!(arena_still_writing, n = at + 1));
        }
        let name = crate::transcript::clean(&member.contender.named());
        let head = arena.head.clone();
        if !sure {
            let snapshot = arena.members[at].snapshot.clone();
            let taking = match crate::worktree::taking(&tree, &start, snapshot.as_deref()) {
                Ok(taking) => taking,
                Err(reason) => return self.active().transcript.note(reason, Tone::Fault),
            };
            if let Some(arena) = &mut self.arena {
                arena.offered = Some((at, taking.now.clone()));
            }
            let shown = |paths: &[String]| {
                let mut shown: Vec<&str> = paths.iter().take(6).map(String::as_str).collect();
                if paths.len() > shown.len() {
                    shown.push("…");
                }
                shown.join(", ")
            };
            let mut detail = tr!(arena_take_files, files = shown(&taking.files));
            if !taking.executable.is_empty() {
                detail = tr!(
                    clause_join,
                    first = detail,
                    second = tr!(arena_take_executable, files = shown(&taking.executable))
                );
            }
            let items = vec![
                crate::picker::Item::plain(tr!(no), tr!(arena_take_no), crate::picker::Pick::Shown),
                crate::picker::Item::plain(
                    tr!(arena_take_yes),
                    detail,
                    crate::picker::Pick::Pane(Request::ArenaTake(at, true)),
                ),
            ];
            let title = tr!(
                arena_take_title,
                n = at + 1,
                name = name,
                count = taking.files.len()
            );
            self.active().picker = Some(crate::picker::Picker::new(title, items, None).guarded());
            return;
        }
        // Only what the owner was shown: the yes names that list's tree.
        let Some(now) = arena
            .offered
            .as_ref()
            .filter(|(offered, _)| *offered == at)
            .map(|(_, now)| now.clone())
        else {
            return self.active().tell(tr!(arena_take_usage));
        };
        match crate::worktree::apply(&tree, &start, &now, &head) {
            Ok(files) => {
                if let Some(arena) = &mut self.arena {
                    arena.taken = Some(at);
                    let home = self.list[self.active].app.parts.home.clone();
                    let win = crate::bench::Record {
                        date: chrono::Local::now().format("%Y-%m-%d").to_string(),
                        set: crate::bench::FREE.to_owned(),
                        task: crate::transcript::cut(&arena.task, 80),
                        model: arena.members[at].contender.named(),
                        passed: true,
                        seconds: arena.members[at].took.map(|took| took.as_secs()),
                        dollars: None,
                        tokens: 0,
                        refusals: arena.members[at].refusals,
                        ended: None,
                    };
                    let _ = crate::bench::keep(&home, &[win]);
                }
                let folder = tree.repo.top.display().to_string();
                self.active().tell(tr!(
                    arena_taken,
                    n = at + 1,
                    name = name,
                    files = files,
                    folder = folder
                ));
            }
            Err(reason) => self.active().transcript.note(reason, Tone::Fault),
        }
    }

    /// `/arena end`: asked first, then every model's pane closed (a turn under way stopped) and its worktree removed
    /// with what it holds.
    fn end_arena(&mut self, sure: bool) {
        let Some(arena) = &self.arena else {
            return self.active().tell(tr!(arena_none));
        };
        if !sure {
            let lost = match arena.taken {
                Some(taken) => tr!(arena_end_taken, n = taken + 1),
                None => tr!(arena_end_nothing_taken).to_owned(),
            };
            let items = vec![
                crate::picker::Item::plain(
                    tr!(no),
                    tr!(arena_end_keep),
                    crate::picker::Pick::Shown,
                ),
                crate::picker::Item::plain(
                    tr!(arena_end_yes),
                    lost,
                    crate::picker::Pick::Pane(Request::ArenaEnd(true)),
                ),
            ];
            let picker = crate::picker::Picker::new(
                tr!(arena_end_title, n = arena.members.len()),
                items,
                None,
            )
            .guarded();
            self.active().picker = Some(picker);
            return;
        }
        // A check cannot be stopped midway: the worktrees go once every check is over.
        if arena.checks.contains(&crate::arena::Check::Running) {
            return self.active().tell(tr!(arena_checking));
        }
        // Git goes over the worktrees only once nothing writes there any more: turns under way are stopped first.
        let working: Vec<usize> = self
            .list
            .iter()
            .enumerate()
            .filter(|(_, pane)| {
                pane.app.working()
                    && arena
                        .members
                        .iter()
                        .any(|member| member.pane == Some(pane.id))
            })
            .map(|(at, _)| at)
            .collect();
        if !working.is_empty() {
            for at in working {
                self.list[at].app.stop_turn();
            }
            return self.active().tell(tr!(arena_stopping));
        }
        let Some(arena) = self.arena.take() else {
            return;
        };
        self.card = None;
        // Ended by the owner mid-bench, the bench ends with it (its start repository once the worktrees are gone);
        // ended by the bench itself, it goes on.
        let stopped = self
            .bench
            .as_mut()
            .and_then(|bench| Some((bench.current.take()?.1, bench.home.clone())));
        if let Some((_, home)) = &stopped {
            for at in 0..arena.members.len() {
                let _ = std::fs::remove_dir_all(check_place(home, &arena, at));
            }
            self.bench = None;
            self.active().tell(tr!(bench_stopped));
        }
        let mut failed = Vec::new();
        for member in &arena.members {
            // A model's pane made the main one (`/swap`) stays, and its worktree with it.
            if member.pane == Some(self.grid.main) {
                if let Some(tree) = &member.tree {
                    failed.push(tr!(arena_main_kept, place = tree.place.display()));
                }
                continue;
            }
            if let Some(id) = member.pane {
                self.drop_pane(id);
            }
            if let Some(tree) = &member.tree
                && let Err(reason) = crate::worktree::remove(tree, true)
            {
                failed.push(reason);
            }
        }
        if let Some((place, _)) = stopped {
            crate::bench::forget(&place);
        }
        if failed.is_empty() {
            self.active().tell(tr!(arena_ended));
        } else {
            self.active()
                .transcript
                .note(failed.join(" · "), Tone::Fault);
        }
    }

    /// Pane `id` taken away without its worktree's question (the arena removes the worktree itself); the keys stay
    /// where they were. The main pane stays.
    fn drop_pane(&mut self, id: u64) {
        let Some(at) = self.list.iter().position(|pane| pane.id == id) else {
            return;
        };
        if id == self.grid.main {
            return;
        }
        self.list[at].tree = None;
        let keys = self.list[self.active].id;
        self.active = at;
        self.return_to = (keys != id).then_some(keys);
        self.remove_active();
        self.return_to = None;
    }

    /// At the end (TUI9e): a window of several panes or with agents in the background keeps how it stood for the
    /// next `glut -c`; one of a single pane forgets the layout it was offered. Nothing is kept while a layout is still
    /// coming back, nor without a pane in the project's own folder to start from.
    pub fn keep_layout(&mut self) {
        if !self.may_split || self.restoring.is_some() {
            return;
        }
        let home = self.list[0].app.parts.home.clone();
        // An arena's model works in a clone that goes with the arena (T61): nothing of it comes back.
        let arena = |at: &usize| self.list[*at].tree.as_ref().is_some_and(|tree| tree.cloned);
        let mut shown: Vec<usize> = (0..self.list.len())
            .filter(|at| !self.list[*at].background && !arena(at))
            .collect();
        let behind: Vec<usize> = (0..self.list.len())
            .filter(|at| self.list[*at].background && !arena(at))
            .collect();
        if shown.len() <= 1 && behind.is_empty() {
            if self.offered {
                crate::layout::forget(&home, &self.cwd);
            }
            return;
        }
        // The first kept pane is the one a window of one pane already is: one in the project's folder.
        let Some(first) = shown.iter().position(|at| self.list[*at].tree.is_none()) else {
            return;
        };
        let first = shown.remove(first);
        shown.insert(0, first);
        let kept = |pane: &Pane| crate::layout::Kept {
            session: pane.app.session.id.clone(),
            role: pane.app.role.clone(),
            agent: pane.background.then(|| pane.name.clone()).flatten(),
            branch: pane.tree.as_ref().map(|tree| tree.branch.clone()),
            into: pane.tree.as_ref().and_then(|tree| tree.into.clone()),
        };
        let layout = crate::layout::Layout {
            panes: shown
                .iter()
                .chain(&behind)
                .map(|at| kept(&self.list[*at]))
                .collect(),
            share: self.grid.share,
        };
        // The end is no place for a fault: a layout that cannot be written is simply not offered next time.
        let _ = crate::layout::save(&home, &self.cwd, &layout);
    }

    /// `glut -c` (TUI9e): a kept layout is offered once, before anything is typed — what comes back said in full,
    /// and no key answers it in its first moment.
    pub fn offer_layout(&mut self) {
        if !self.may_split {
            return;
        }
        let home = self.list[0].app.parts.home.clone();
        let Some(layout) = crate::layout::load(&home, &self.cwd) else {
            return;
        };
        self.offered = true;
        let items = vec![
            crate::picker::Item::plain(
                format!("yes: {}", layout.summary()),
                layout.details(),
                crate::picker::Pick::Pane(Request::Restore),
            ),
            crate::picker::Item::plain(
                tr!(layout_no),
                tr!(layout_no_detail),
                crate::picker::Pick::Shown,
            ),
        ];
        self.active().picker =
            Some(crate::picker::Picker::new(tr!(layout_question), items, None).guarded());
    }

    /// The kept layout back: the first pane goes on with the first kept session, the others are built one by one
    /// ([`Panes::restore_step`]), and the split takes its kept shape once they stand.
    fn restore(&mut self) {
        let home = self.list[0].app.parts.home.clone();
        let Some(layout) = crate::layout::load(&home, &self.cwd) else {
            return self.active().tell(tr!(no_layout));
        };
        if self.list.len() > 1 || self.building.is_some() || self.restoring.is_some() {
            return self.active().tell(tr!(layout_one_pane_only));
        }
        // Another session opened under a running turn would take its answer.
        if self.active().working() {
            return self.active().tell(tr!(layout_waits));
        }
        let first = layout.panes[0].clone();
        open_kept(self.active(), &first.session);
        if let Err(reason) = self.active().set_role(first.role.as_deref()) {
            self.active().transcript.note(reason, Tone::Fault);
        }
        let mut ids = vec![None; layout.panes.len()];
        ids[0] = Some(self.list[0].id);
        self.restoring = Some(Restoring {
            left: (1..layout.panes.len()).collect(),
            ids,
            building: None,
            layout,
        });
    }

    /// The next kept pane built — or, with none left, the split shaped as it stood.
    fn restore_step(&mut self, host: &Arc<dyn Host>) {
        let Some(restoring) = &mut self.restoring else {
            return;
        };
        let Some(at) = restoring.left.pop_front() else {
            return self.restore_shape();
        };
        let kept = restoring.layout.panes[at].clone();
        // Every kept pane is built from the first, in the project's folder — not from the one built last.
        if let Some(first) = restoring.ids[0]
            && let Some(at) = self.list.iter().position(|pane| pane.id == first)
        {
            self.active = at;
        }
        // The worktree is found again where Glut puts it, by its branch alone.
        let place = match &kept.branch {
            None => Place::Here,
            Some(branch) => {
                let home = self.active().parts.home.clone();
                let found = crate::worktree::repo_of(&self.cwd).and_then(|repo| {
                    repo.ok_or_else(|| tr!(no_repository_now).to_owned())
                        .and_then(|repo| {
                            crate::worktree::reopen(&home, &repo, branch, kept.into.clone())
                        })
                });
                match found {
                    Ok(tree) => Place::Kept(tree),
                    Err(reason) => {
                        self.active().transcript.note(
                            tr!(kept_pane_away, branch = branch, reason = reason),
                            Tone::Fault,
                        );
                        return;
                    }
                }
            }
        };
        self.restore_next = Some(kept.session);
        self.start_as(false, place, kept.role, kept.agent, None, host);
        // A pane that could not start leaves no building: its kept place stays empty.
        self.restore_next = None;
        if self.building.is_some()
            && let Some(restoring) = &mut self.restoring
        {
            restoring.building = Some(at);
        }
    }

    /// Every kept pane that came back stands where it stood, the first the main one; with one missing, the panes stay
    /// as they were built.
    fn restore_shape(&mut self) {
        let Some(restoring) = self.restoring.take() else {
            return;
        };
        let on_screen = restoring
            .layout
            .panes
            .iter()
            .filter(|kept| kept.agent.is_none())
            .count();
        let ids: Option<Vec<u64>> = restoring.ids[..on_screen].iter().copied().collect();
        // The kept shape only where it holds exactly the panes on the screen now: the owner may have closed, hidden
        // or split one meanwhile.
        let mut now: Vec<u64> = self
            .list
            .iter()
            .filter(|pane| !pane.background)
            .map(|pane| pane.id)
            .collect();
        now.sort_unstable();
        let ids = ids.filter(|ids| {
            let mut kept = ids.clone();
            kept.sort_unstable();
            kept == now
        });
        match ids {
            Some(ids) => {
                // The keys stay with the pane that has them, wherever it stands now.
                let keys = self.list[self.active].id;
                self.grid = Grid {
                    main: ids[0],
                    others: ids[1..].to_vec(),
                    share: restoring.layout.share,
                };
                self.reorder();
                self.active = self.at(keys);
                self.active().tell(tr!(layout_back));
            }
            None => self.active().tell(tr!(layout_back_partly)),
        }
    }

    /// `/bg`: a session with no room on the screen (T36e), named `name` or after its role (`reviewer`, a second
    /// `reviewer-2`), built as a pane is — on the active pane's provider and model, in build.
    fn start_background(
        &mut self,
        name: Option<String>,
        role: Option<String>,
        place: Place,
        sure: bool,
        host: &Arc<dyn Host>,
    ) {
        let background = self.list.iter().filter(|pane| pane.background).count();
        self.background_sure |= sure;
        if background >= BACKGROUND_ASKED && !self.background_sure {
            let picker = more_picker(background + 1, name, role, place);
            self.active().picker = Some(picker);
            return;
        }
        let base = name
            .or_else(|| role.clone())
            .filter(|base| is_name(base))
            .unwrap_or_else(|| "agent".to_owned());
        let name = self.free_name(base);
        self.start_as(false, place, role, Some(name), None, host);
    }

    /// `base`, or `base-2`, `base-3`, … where a pane or an agent is named so or plays a role of that name.
    fn free_name(&self, base: String) -> String {
        let taken = |candidate: &str| {
            self.list.iter().any(|pane| {
                pane.name.as_deref() == Some(candidate)
                    || pane.app.role.as_deref() == Some(candidate)
            })
        };
        if taken(&base) {
            (2..)
                .map(|n| format!("{base}-{n}"))
                .find(|candidate| !taken(candidate))
                .unwrap_or(base)
        } else {
            base
        }
    }

    /// `/loop <interval> … <task>` (T60a): a background agent of its own runs the task, its first round at once —
    /// named `loop` or after its role.
    fn start_loop(&mut self, asked: crate::looping::Asked, host: &Arc<dyn Host>) {
        // Loops spend unattended: their own cap, whatever the owner said yes to for `/bg`.
        let background = self.list.iter().filter(|pane| pane.background).count();
        let loops = self
            .list
            .iter()
            .filter(|pane| pane.looping.is_some())
            .count();
        if (background >= BACKGROUND_ASKED && !self.background_sure) || loops >= BACKGROUND_ASKED {
            return self
                .active()
                .tell(tr!(loop_too_many, n = background.max(loops)));
        }
        // A role narrows: a loop of a pane that plays one plays it too, never another.
        let role = match (asked.role.clone(), self.active().role.clone()) {
            (Some(theirs), Some(ours)) if theirs != ours => {
                return self.active().tell(tr!(loop_role_kept, role = ours));
            }
            (theirs, ours) => theirs.or(ours),
        };
        let base = role
            .clone()
            .filter(|role| is_name(role))
            .unwrap_or_else(|| "loop".to_owned());
        let name = self.free_name(base);
        self.start_as(false, Place::Here, role, Some(name), Some(asked), host);
    }

    /// The loops' rounds (T60a): a round is over once its agent rests again — the next one set from its end, or the
    /// loop ended — and a round due starts where its agent is free.
    fn step_loops(&mut self, now: Instant) {
        let mut ended = Vec::new();
        for (at, pane) in self.list.iter_mut().enumerate() {
            let Some(looping) = &mut pane.looping else {
                continue;
            };
            let app = &mut pane.app;
            if app.working() || app.waits_for_owner() || app.picker.is_some() {
                continue;
            }
            if looping.running() {
                if let Some(why) =
                    looping.round_ended(app.cost, crate::looping::tokens_of(&app.usage), now)
                {
                    ended.push((at, why));
                }
            } else if looping.next.is_some_and(|next| next <= now) {
                let (task, fresh) = (looping.asked.task.clone(), looping.asked.fresh);
                // Its limits stand before its turn starts and reads them.
                looping.round_starts();
                // A fresh round's new conversation counts its cost from nothing.
                app.loop_round(&task, fresh && looping.rounds > 0);
                // A task that starts nothing (a skill not there, a list opened) would never fail on its own.
                if !app.working() {
                    app.picker = None;
                    ended.push((
                        at,
                        crate::looping::Ended::Failed(tr!(loop_no_turn).to_owned()),
                    ));
                    continue;
                }
                looping.round_began(app.cost, crate::looping::tokens_of(&app.usage));
            }
        }
        for (at, why) in ended {
            self.end_loop(at, why);
        }
    }

    /// When the next round of a loop is due: the screen wakes for it.
    pub fn loop_due(&self) -> Option<Instant> {
        // Only an agent free to start it: one busy is woken by the report or the key that frees it, and a time long
        // past would wake the screen again and again meanwhile. One past for a free agent wakes it at once.
        self.list
            .iter()
            .filter(|pane| {
                let app = &pane.app;
                !(app.working() || app.waits_for_owner() || app.picker.is_some())
            })
            .filter_map(|pane| pane.looping.as_ref()?.next)
            .min()
    }

    /// The loop of the pane at `at` ends, why said where the owner sees it — a tile but for the owner's own stop; its
    /// agent stays, resting, to be read or closed.
    fn end_loop(&mut self, at: usize, why: crate::looping::Ended) {
        use crate::tiles::{Goes, Stand, Tile};
        let Some(looping) = self.list[at].looping.take() else {
            return;
        };
        looping.link.stop();
        self.list[at].app.stop_turn();
        self.list[at].app.loop_ended = Some(tr!(
            loop_card_ended,
            rounds = looping.rounds,
            most = looping.limits.rounds
        ));
        let pane = &self.list[at];
        let name = pane.name.clone().unwrap_or_else(|| "loop".to_owned());
        let goes = if pane.background {
            Goes::Behind {
                name: name.clone(),
                asks: false,
            }
        } else {
            Goes::Pane(pane.id)
        };
        let said = why.said();
        // A round put back into the input (a refused turn) is no word of the owner's.
        if self.list[at].app.input.text() == looping.asked.task {
            self.list[at].app.input.take();
        }
        // `notify` told of the last round already; the owner's own stop needs no telling.
        if why != crate::looping::Ended::Stopped && !looping.asked.notify {
            let stand = if why.well() {
                Stand::Done
            } else {
                Stand::Failed
            };
            let who = self.shown_label(at);
            self.tiles.push(Tile::new(who, stand, &said, goes));
            let ran = looping.began.elapsed();
            let occasion = crate::notify::Occasion::Ended {
                ran,
                answered: why.well(),
            };
            let choice = crate::notify::choice();
            if self.notifier.focused != Some(true)
                && crate::notify::worth_a_message(choice, self.notifier.focused, &occasion, ran)
            {
                self.notifier.push(Some(name.clone()), &occasion);
            }
        }
        let cost = looping.spent_shown();
        self.active().tell(tr!(
            loop_ended,
            name = name,
            why = said,
            rounds = looping.rounds,
            cost = cost
        ));
    }

    /// `/loop stop [name]`: that loop, or the only one.
    fn stop_loop(&mut self, name: Option<&str>) {
        let loops: Vec<usize> = (0..self.list.len())
            .filter(|at| self.list[*at].looping.is_some())
            .collect();
        let at = match name {
            Some(name) => loops
                .iter()
                .copied()
                .find(|at| self.list[*at].name.as_deref() == Some(name)),
            None if loops.is_empty() => return self.active().tell(tr!(loop_none)),
            None if loops.len() == 1 => Some(loops[0]),
            None => return self.active().tell(tr!(loop_which)),
        };
        match at {
            Some(at) => self.end_loop(at, crate::looping::Ended::Stopped),
            None => {
                let name = crate::transcript::clean(name.unwrap_or_default());
                self.active().tell(tr!(loop_unknown, name = name));
            }
        }
    }

    /// `/loop`: the loops running, each with its interval, its rounds, when the next one starts and its cost.
    fn list_loops(&mut self) {
        let now = Instant::now();
        let lines: Vec<String> = self
            .list
            .iter()
            .filter_map(|pane| {
                let looping = pane.looping.as_ref()?;
                Some(looping.line(pane.name.as_deref().unwrap_or("loop"), now))
            })
            .collect();
        if lines.is_empty() {
            return self.active().tell(tr!(loop_none));
        }
        let mut text = tr!(loops_heading).to_owned();
        for line in lines {
            text.push_str("\n  ");
            text.push_str(&line);
        }
        self.active().tell(text);
    }

    /// `/show <name>`: a background agent brought forward as a pane, the last in the grid.
    fn show(&mut self, name: &str) {
        let Some(at) = self.resolve_background(name) else {
            return self.active().tell(tr!(
                no_background_agent,
                name = crate::transcript::clean(name)
            ));
        };
        if self.visible() >= MOST {
            return self.active().tell(tr!(most_panes_hide, n = MOST));
        }
        if !self.room_for_one_more() {
            return self.active().tell(tr!(no_room_show));
        }
        let id = self.list[at].id;
        // Panes leave little room beside them: the side panel goes, as when a pane is split off.
        self.active().panel = Some(false);
        self.grid.others.push(id);
        self.list[at].background = false;
        self.peek = None;
        self.reorder();
        let at = self.at(id);
        self.focus(at);
    }

    /// `/close <name>`: a background agent ended where it stands, its turn stopped — one in a worktree is shown
    /// first, where how its worktree ends is asked.
    fn end_background(&mut self, name: &str) {
        let Some(at) = self.resolve_background(name) else {
            return self.active().tell(tr!(
                no_background_agent,
                name = crate::transcript::clean(name)
            ));
        };
        let label = self.shown_label(at);
        if self.list[at].tree.is_some() {
            return self.active().tell(tr!(agent_in_worktree, name = label));
        }
        let keys = self.list[self.active].id;
        let mut pane = self.list.remove(at);
        let stopped = pane.app.working();
        pane.app.stop_turn();
        if self.peek == Some(pane.id) {
            self.peek = None;
        }
        self.active = self.at(keys);
        let kept = if stopped {
            tr!(closed_stopped)
        } else {
            tr!(closed_resume)
        };
        self.active()
            .tell(tr!(agent_ended, name = label, kept = kept));
    }

    /// `/hide`: the active pane goes to the background with its memory; it keeps a name to be reached by.
    fn hide(&mut self) {
        if self.visible() <= 1 {
            return self.active().tell(tr!(last_pane_stays));
        }
        if self.list[self.active].id == self.grid.main {
            return self.active().tell(tr!(main_pane_stays));
        }
        let at = self.active;
        let number = self.list[at].number;
        if self.list[at].name.is_none() {
            let name = self.list[at]
                .app
                .role
                .clone()
                .filter(|role| self.resolve(role, at).is_none())
                .unwrap_or_else(|| format!("pane-{number}"));
            // Another agent may carry that name already: the next free one.
            let taken = |candidate: &str| {
                self.list.iter().enumerate().any(|(other, pane)| {
                    other != at
                        && (pane.name.as_deref() == Some(candidate)
                            || pane.app.role.as_deref() == Some(candidate))
                })
            };
            let name = if taken(&name) {
                (2..)
                    .map(|n| format!("{name}-{n}"))
                    .find(|candidate| !taken(candidate))
                    .unwrap_or(name)
            } else {
                name
            };
            self.list[at].name = Some(name);
        }
        let id = self.list[at].id;
        self.grid.remove(id);
        self.list[at].background = true;
        // Where it stood is another pane's now: the mouse must not find it there.
        self.list[at].area = Rect::default();
        let next = (0..self.list.len())
            .find(|other| !self.list[*other].background)
            .map(|other| self.list[other].id)
            .unwrap_or(id);
        self.reorder();
        let label = self.shown_label(self.at(id));
        self.focus(self.at(next));
        // Alone again: the side panel comes back as the owner keeps it.
        if !self.split() {
            let home = self.active().parts.home.clone();
            self.active().panel = crate::prefs::Prefs::load(&home).panel;
        }
        self.active().tell(tr!(hidden_works_on, name = label));
    }

    /// Moves the keys to pane `at`; what it asks counts as shown from now.
    fn focus(&mut self, at: usize) {
        // The keys never go to a pane drawn nowhere.
        if self.list[at].background {
            return;
        }
        self.active = at;
        let id = self.list[at].id;
        if id != self.grid.main {
            self.front = id;
        }
        self.active().shown_again();
    }

    /// Where a tile's click goes (T69): the Agents tab — at the subagent, in its pane —, or the pane itself.
    fn open_tile(&mut self, goes: crate::tiles::Goes) {
        use crate::tiles::Goes;
        let at = |id: u64| self.list.iter().position(|pane| pane.id == id);
        match goes {
            // A question is answered where its agent stands: brought forward, its ask card in front of the owner.
            Goes::Behind { name, asks: true } => {
                self.show(&name);
                // No room to bring it forward (said already): its card, asking, is where the question stays seen.
                if self.resolve_background(&name).is_some() {
                    self.active().open_agents(crate::agents::Row::Behind(name));
                }
            }
            Goes::Behind { name, asks: false } => {
                self.active().open_agents(crate::agents::Row::Behind(name))
            }
            Goes::Subagent { pane, path } => {
                if let Some(at) = at(pane) {
                    self.focus(at);
                    self.active().open_agents(crate::agents::Row::Sub(path));
                }
            }
            Goes::Pane(pane) => {
                if let Some(at) = at(pane) {
                    self.focus(at);
                }
            }
            Goes::Runs(plan) => self.active().open_runs(&plan),
        }
    }

    /// The plans' runs that ended (T60b): at the first look, what ran since Glut last started, said in the pane in
    /// front; later each one ended since as a tile. Glut on its own only: a host's window shows its own.
    pub fn watch_runs(&mut self, now: chrono::DateTime<chrono::Utc>) {
        use crate::tiles::{Goes, Stand, Tile};
        if !self.may_split {
            return;
        }
        let home = self.active().parts.home.clone();
        let folder = std::fs::metadata(crate::schedule::dir(&home))
            .and_then(|meta| meta.modified())
            .ok();
        let Some(seen) = &mut self.runs_seen else {
            let since = crate::prefs::Prefs::load(&home).runs_seen;
            self.runs_seen = Some(RunsSeen { from: now, folder });
            let _ = crate::prefs::set_runs_seen(&home, now);
            let Some(since) = since else {
                return;
            };
            let ended = crate::schedule::ended_since(&home, since);
            // What the note tells is not tiled again, ended a moment after `now` as it may be.
            if let (Some(seen), Some((_, last))) = (&mut self.runs_seen, ended.last()) {
                seen.from = seen.from.max(last.ended);
            }
            if ended.is_empty() {
                return;
            }
            let mut told: Vec<(String, usize, crate::schedule::Outcome)> = Vec::new();
            for (name, record) in ended {
                match told.iter_mut().find(|(known, ..)| *known == name) {
                    Some((_, count, outcome)) => {
                        *count += 1;
                        *outcome = record.outcome;
                    }
                    None => told.push((name, 1, record.outcome)),
                }
            }
            let runs: Vec<String> = told
                .into_iter()
                .map(|(name, n, outcome)| {
                    tr!(runs_since_one, name = name, n = n, outcome = outcome.said())
                })
                .collect();
            return self.active().tell(tr!(runs_since, runs = runs.join(", ")));
        };
        if seen.folder == folder {
            return;
        }
        seen.folder = folder;
        let ended = crate::schedule::ended_since(&home, seen.from);
        for (name, record) in ended {
            seen.from = seen.from.max(record.ended);
            let stand = if record.outcome.well() {
                Stand::Done
            } else {
                Stand::Failed
            };
            let line = if record.said.is_empty() {
                record.outcome.said().to_owned()
            } else {
                record.said.clone()
            };
            self.tiles
                .push(Tile::new(name.clone(), stand, &line, Goes::Runs(name)));
            self.redraw = true;
        }
        let _ = crate::prefs::set_runs_seen(&home, now);
    }

    /// `/swap [n]`: the main pane and pane `n` trade places — without a number the asking pane, or with only two the
    /// other one; the keys stay with the pane that has them.
    fn swap(&mut self, with: Option<String>) {
        if !self.split() {
            return self.active().tell(tr!(one_pane_swap));
        }
        let main = self.grid.main;
        let target = match &with {
            Some(word) => self
                .resolve(word, usize::MAX)
                .filter(|at| !self.list[*at].background),
            None if self.list[self.active].id != main => Some(self.active),
            None if self.grid.others.len() == 1 => Some(self.at(self.grid.others[0])),
            None => return self.active().tell(tr!(swap_which)),
        };
        let Some(target) = target else {
            let word = with.as_deref().unwrap_or_default();
            return self
                .active()
                .tell(tr!(no_pane_shown, name = crate::transcript::clean(word)));
        };
        let id = self.list[target].id;
        if id == main {
            return self.active().tell(tr!(main_already));
        }
        let keys = self.list[self.active].id;
        self.grid.swap(id);
        // The keys' pane stays the one shown in front where the right side holds tabs.
        self.front = if keys == id { main } else { keys };
        self.reorder();
        self.active = self.at(keys);
        let number = self.list[self.at(id)].number;
        self.active().tell(tr!(main_now, n = number));
    }

    /// `⌃X }` / `⌃X {`: the main pane wider or narrower by `by` percent, within its bounds.
    fn widen(&mut self, by: i16) {
        if !self.split() {
            return;
        }
        let share = i32::from(self.grid.share) + i32::from(by);
        self.grid.share = share.clamp(i32::from(SHARE_LEAST), i32::from(SHARE_MOST)) as u16;
    }

    /// A click on a tab shows its pane and gives it the keys; the border to the main pane follows the mouse while
    /// dragged. `true` when the mouse was the frame's, not a pane's.
    fn on_frame(&mut self, mouse: &crossterm::event::MouseEvent) -> bool {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // An `Up` lost while the window was one pane or under a peek ends a drag here.
                self.dragging = false;
                if let Some(tabs) = &self.placed.tabs
                    && contains(tabs.area, mouse.column, mouse.row)
                {
                    let names: Vec<(u64, String)> = tabs
                        .panes
                        .iter()
                        .map(|id| (*id, self.tab_name(*id)))
                        .collect();
                    // A tab of the last frame whose pane has gone since is no pane to go to.
                    if let Some(id) = tab_under(tabs.area, &names, tabs.shown, mouse.column)
                        && let Some(at) = self.list.iter().position(|pane| pane.id == id)
                    {
                        self.focus(at);
                    }
                    return true;
                }
                if self
                    .placed
                    .border
                    .is_some_and(|border| contains(border, mouse.column, mouse.row))
                {
                    self.dragging = true;
                    return true;
                }
                false
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let room = self.room;
                if room.width > 0 {
                    let column = u32::from(mouse.column.saturating_sub(room.x));
                    let share = (column * 100 / u32::from(room.width)) as u16;
                    self.grid.share = share.clamp(SHARE_LEAST, SHARE_MOST);
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
                true
            }
            _ => false,
        }
    }

    /// A tab's words: the pane's number and, if it plays one, its role, cut short.
    pub fn tab_name(&self, id: u64) -> String {
        // A role's name may be long: a tab keeps room for the others.
        const MOST_ROLE: usize = 12;
        let Some(pane) = self.list.iter().find(|pane| pane.id == id) else {
            return String::new();
        };
        match &pane.app.role {
            Some(role) => format!(
                "{} {}",
                pane.number,
                crate::transcript::cut(&crate::transcript::clean(role), MOST_ROLE)
            ),
            None => pane.number.to_string(),
        }
    }

    /// Whether one more pane beside the main one keeps each at least [`PANE_MIN_COLUMNS`] and [`PANE_MIN_ROWS`] in
    /// the room the last frame had; before the first frame there is no telling, and it may.
    fn room_for_one_more(&self) -> bool {
        self.room.width == 0
            || self
                .grid
                .fits(self.room, self.grid.others.len() + 1, PANE_MIN_COLUMNS)
    }

    /// Closes a peek; a question that came up under it counts as shown only from now.
    fn close_peek(&mut self) {
        self.peek = None;
        self.active().shown_again();
    }

    /// The list in the layout's reading order, so `⌃P` goes round as the panes stand.
    fn reorder(&mut self) {
        let order = self.grid.order();
        self.list.sort_by_key(|pane| {
            order
                .iter()
                .position(|id| *id == pane.id)
                .unwrap_or(usize::MAX)
        });
    }

    /// Where the pane of session `id` stands in the list (it is there: every caller just put it or kept it).
    fn at(&self, id: u64) -> usize {
        self.list.iter().position(|pane| pane.id == id).unwrap_or(0)
    }

    /// The narrowest the panes may share and keep each at least `least` columns wide.
    pub fn needs(&self, least: u16) -> u16 {
        self.grid.needs(least)
    }

    /// Each pane's area in `area`, in the list's order (none for a pane drawn only as a tab), and where the frame's
    /// lines and tabs stand — kept for the mouse and for the room a new pane would share.
    pub fn areas(&mut self, area: Rect) -> (Vec<Rect>, Placed) {
        if !self.split() {
            self.room = area;
            self.placed = Placed::default();
            self.dragging = false;
            return (vec![area], Placed::default());
        }
        let active = self.list[self.active].id;
        let placed = self.grid.place(area, self.front, active);
        let areas = self
            .list
            .iter()
            .map(|pane| {
                placed
                    .areas
                    .iter()
                    .find(|(id, _)| *id == pane.id)
                    .map_or(Rect::default(), |(_, area)| *area)
            })
            .collect();
        self.room = area;
        self.placed = placed.clone();
        (areas, placed)
    }
}

/// Where a new pane in a git repository works: this folder, or a worktree whose branch the owner may rename first
/// (the command goes into the input).
fn where_picker(
    resume: bool,
    role: Option<&str>,
    repo: &crate::worktree::Repo,
    number: u8,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let branch = crate::worktree::proposed_branch(repo, number);
    let resume_word = if resume { " resume" } else { "" };
    let role_words = role.map_or(String::new(), |role| format!(" as {role}"));
    let items = vec![
        Item::plain(
            tr!(where_here),
            tr!(where_here_detail),
            Pick::Pane(Request::Split {
                resume,
                place: Place::Here,
                role: role.map(str::to_owned),
            }),
        ),
        Item::plain(
            tr!(where_tree),
            tr!(where_tree_detail, branch = branch),
            Pick::Input(format!("/split{resume_word}{role_words} tree {branch}")),
        ),
    ];
    Picker::new(tr!(where_title, n = number), items, None).guarded()
}

/// One more agent in the background than [`BACKGROUND_ASKED`] (T36f): asked once a window, no the default.
fn more_picker(
    count: usize,
    name: Option<String>,
    role: Option<String>,
    place: Place,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let items = vec![
        Item::plain(tr!(no), tr!(more_no_detail), Pick::Shown),
        Item::plain(
            tr!(more_yes),
            tr!(more_yes_detail),
            Pick::Pane(Request::Background {
                name,
                role,
                place,
                sure: true,
            }),
        ),
    ];
    Picker::new(tr!(more_title, n = count), items, None).guarded()
}

/// The tab under `column` on a line of tabs drawn as [`tab_spots`] lays them out.
fn tab_under(area: Rect, names: &[(u64, String)], shown: u64, column: u16) -> Option<u64> {
    tab_spots(area, names, shown)
        .into_iter()
        .find(|(_, spot)| column >= spot.x && column < spot.x + spot.width)
        .map(|(id, _)| id)
}

/// Where each tab stands on its line: ` name ` after ` name `, from the left, as many as fit — starting late enough
/// that the tab of `shown` is among them.
pub fn tab_spots(area: Rect, names: &[(u64, String)], shown: u64) -> Vec<(u64, Rect)> {
    let laid = |from: usize| {
        let mut x = area.x;
        let end = area.x + area.width;
        let mut spots = Vec::new();
        for (id, name) in &names[from..] {
            let width = unicode_width::UnicodeWidthStr::width(name.as_str()) as u16 + 2;
            if x + width > end {
                break;
            }
            spots.push((*id, Rect::new(x, area.y, width, 1)));
            x += width + 1;
        }
        spots
    };
    (0..names.len())
        .map(laid)
        .find(|spots| spots.iter().any(|(id, _)| *id == shown))
        .unwrap_or_else(|| laid(0))
}

/// How a worktree's pane ends, with what it holds said.
fn leaving_picker(
    tree: &crate::worktree::Tree,
    standing: Result<crate::worktree::Standing, String>,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let held = match &standing {
        Ok(standing) => standing.said(),
        Err(_) => tr!(holds_unreadable).to_owned(),
    };
    let into = tree.into.as_deref().unwrap_or(tr!(no_branch_merge));
    let items = vec![
        Item::plain(
            tr!(leave_keep),
            tr!(leave_keep_detail, branch = tree.branch, held = held),
            Pick::Pane(Request::CloseAs(Leaving::Keep)),
        ),
        Item::plain(
            tr!(leave_take_over),
            tr!(
                leave_take_over_detail,
                branch = tree.branch,
                held = held,
                into = into
            ),
            Pick::Pane(Request::CloseAs(Leaving::TakeOver)),
        ),
        Item::plain(
            tr!(leave_discard),
            tr!(leave_discard_detail, branch = tree.branch, held = held),
            Pick::Pane(Request::CloseAs(Leaving::Discard)),
        ),
    ];
    Picker::new(tr!(leave_title), items, None).guarded()
}

/// The second question before work goes.
fn discard_picker(
    tree: &crate::worktree::Tree,
    standing: &Result<crate::worktree::Standing, String>,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let items = vec![
        Item::plain(tr!(no), tr!(discard_no_detail), Pick::Shown),
        Item::plain(
            tr!(discard_yes),
            match standing {
                Ok(standing) => tr!(discard_lost, branch = tree.branch, held = standing.said()),
                Err(reason) => tr!(discard_whatever, branch = tree.branch, reason = reason),
            },
            Pick::Pane(Request::CloseAs(Leaving::DiscardSure)),
        ),
    ];
    Picker::new(tr!(discard_title, branch = tree.branch), items, None).guarded()
}

/// The new pane's session: the owner's, in the active pane's folder, on its provider and model, with the first
/// session's servers, starting in build (the owner's flow: the first pane plans, the second builds).
/// Where member `at`'s bench check runs: in `~/.glut`, by its worktree's branch.
fn check_place(
    home: &std::path::Path,
    arena: &crate::arena::Arena,
    at: usize,
) -> std::path::PathBuf {
    crate::bench::checks_dir(home).join(arena.branch(at).replace('/', "-"))
}

/// Each arena model's worktree written down as git's tree id, once: what its card, numbers and a take read.
fn snapshots(arena: &mut crate::arena::Arena) {
    for member in &mut arena.members {
        if member.snapshot.is_none()
            && let Some(tree) = &member.tree
        {
            member.snapshot = crate::worktree::snapshot(tree).ok();
        }
    }
}

fn launch_beside(app: &App, servers: Servers) -> Launch {
    let mut launch = Launch::owner(
        app.parts.config.cwd.clone(),
        Some(app.engine.provider.clone()),
        Some(app.engine.model_name().to_owned()),
    );
    launch.servers = servers;
    launch.gate.mode = Some(glut::permissions::Mode::AcceptEdits);
    launch
}

/// Hands one session's reports to the screen's channel with its id, until either side is gone.
fn forward(
    id: u64,
    mut from: mpsc::UnboundedReceiver<Report>,
    to: mpsc::UnboundedSender<(u64, Report)>,
) {
    tokio::spawn(async move {
        while let Some(report) = from.recv().await {
            if to.send((id, report)).is_err() {
                break;
            }
        }
    });
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.x + area.width && row >= area.y && row < area.y + area.height
}

/// `2` or `2 and 3`.
#[cfg(test)]
fn numbers(numbers: &[u8]) -> String {
    let words: Vec<String> = numbers.iter().map(u8::to_string).collect();
    listed(&words)
}

/// Words read as a list: "a", "a and b", "a, b and c".
fn listed(words: &[String]) -> String {
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            tr!(list_and, rest = rest.join(tr!(list_comma)), last = last)
        }
        _ => words.join(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
    use tokio::sync::oneshot;

    fn app() -> (App, mpsc::UnboundedReceiver<Report>) {
        let (parts, engine) = session::for_tests();
        let (send, receive) = mpsc::unbounded_channel();
        (App::new(Arc::new(parts), engine, send), receive)
    }

    /// Two panes, the second built beside the main one, without starting a real session.
    fn split() -> (Panes, Inlets) {
        let (first, reports) = app();
        let (mut panes, inlets) = Panes::new(first, reports, true, Servers::Owner);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 2,
            number: 2,
            resume: false,
            tree: None,
            role: None,
            background: None,
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        (panes, inlets)
    }

    #[tokio::test]
    async fn send_alone_lists_whom_to_write_to_and_a_pick_puts_the_address_into_the_input() {
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        let (mut panes, _inlets) = split();
        panes.focus(0);
        panes.active().pane_request = Some(parse_send("").unwrap());
        panes.settle(&host);
        let picker = panes.list[0]
            .app
            .picker
            .as_ref()
            .expect("the list of whom to write to");
        assert_eq!(
            picker.chosen(),
            Some(crate::picker::Pick::WriteTo("2".into())),
            "the other pane, by its number"
        );
    }

    #[tokio::test]
    async fn send_to_another_window_from_a_single_pane_reaches_it() {
        use crate::sessions::By;
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        let (first, reports) = app();
        let (mut panes, _inlets) = Panes::new(first, reports, true, Servers::Owner);
        let home = std::env::temp_dir().join(format!("glut-send-window-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let (link, _inlet) = crate::sessions::Link::start(&home, std::path::Path::new("/x/alpha"))
            .await
            .unwrap();
        let mut letters = crate::sessions::fake_window(&link, "beta-xy");
        link.look().await;
        panes.link = Some(link);
        panes.sync();
        assert!(
            panes.list[0].app.address.is_some(),
            "the header names this window"
        );
        // What the `/send` list puts into the input, and the message typed behind it.
        panes.active().pane_request = Some(parse_send("beta-xy hello there").unwrap());
        panes.settle(&host);
        let letter = tokio::time::timeout(Duration::from_secs(5), letters.recv())
            .await
            .expect("the other window took it")
            .unwrap();
        assert_eq!(letter.text, "hello there");
        assert_eq!(letter.by, By::Owner);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn close_takes_a_pane_by_its_number_from_elsewhere_and_alone_lists_what_could_close() {
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        // From the main pane, which never closes: `/close` alone lists pane 2 rather than refusing.
        let (mut panes, _inlets) = split();
        panes.focus(0);
        panes.active().pane_request = Some(Request::CloseAsked);
        panes.settle(&host);
        assert_eq!(panes.list.len(), 2);
        assert!(
            panes.list[0].app.picker.is_some(),
            "the list of what could close"
        );
        // `/close 2` closes it from here, and the keys stay where they were.
        panes.list[0].app.picker = None;
        panes.active().pane_request = Some(Request::End("2".into()));
        panes.settle(&host);
        assert_eq!(panes.list.len(), 1);
        assert_eq!(panes.active, 0);
        // A word naming no single pane opens the list too.
        let (mut panes, _inlets) = split();
        panes.focus(0);
        panes.active().pane_request = Some(Request::End("all-rounder".into()));
        panes.settle(&host);
        assert_eq!(panes.list.len(), 2);
        assert!(panes.list[0].app.picker.is_some());
        // In pane 2, the only one that could close, `/close` closes it as it always did.
        let (mut panes, _inlets) = split();
        panes.focus(1);
        panes.active().pane_request = Some(Request::CloseAsked);
        panes.settle(&host);
        assert_eq!(panes.list.len(), 1);
    }

    /// One pane and an agent in the background named `name`, built without starting a real session.
    fn with_background(name: &str) -> (Panes, Inlets) {
        let (first, reports) = app();
        let (mut panes, inlets) = Panes::new(first, reports, true, Servers::Owner);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 2,
            number: 2,
            resume: false,
            tree: None,
            role: None,
            background: Some(name.to_owned()),
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        (panes, inlets)
    }

    fn numbers_of(panes: &Panes) -> Vec<u8> {
        panes.list.iter().map(|pane| pane.number).collect()
    }

    fn press(panes: &mut Panes, code: KeyCode, modifiers: KeyModifiers) {
        panes.on_terminal(TermEvent::Key(KeyEvent::new(code, modifiers)));
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
    }

    #[tokio::test]
    async fn a_new_pane_takes_the_next_number_beside_the_main_one_and_the_keys() {
        let (mut panes, _inlets) = split();
        assert_eq!(
            (numbers_of(&panes), panes.active, panes.needs(60)),
            (vec![1, 2], 1, 121)
        );
        let (areas, placed) = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(
            (areas[0].width, areas[1].x, areas[1].width),
            (50, 51, 50),
            "a column parts them"
        );
        assert_eq!(placed.lines, [Rect::new(50, 0, 1, 40)]);
        assert_eq!(placed.border, Some(Rect::new(50, 0, 1, 40)));
    }

    #[tokio::test]
    async fn ctrl_l_draws_the_screen_anew_and_types_nothing() {
        let (mut panes, _inlets) = split();
        panes.redraw = false;
        press(&mut panes, KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert!(panes.redraw);
        assert!(panes.active().input.is_empty());
    }

    /// Pane `number` built into the grid, as `on_built` makes it.
    fn add(panes: &mut Panes, number: u8) {
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: u64::from(number),
            number,
            resume: false,
            tree: None,
            role: None,
            background: None,
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
    }

    #[tokio::test]
    async fn panes_fill_the_grid_beside_the_main_one_and_closing_one_closes_the_gap() {
        let (mut panes, _inlets) = split();
        // 1 | (2 over 3): the main pane keeps its full height.
        add(&mut panes, 3);
        assert_eq!(numbers_of(&panes), vec![1, 2, 3]);
        let (areas, placed) = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(areas[0], Rect::new(0, 0, 50, 40));
        assert_eq!(
            (areas[1], areas[2]),
            (Rect::new(51, 0, 50, 20), Rect::new(51, 20, 50, 20))
        );
        assert_eq!(placed.lines.len(), 1);
        // From any pane a new one goes last into the grid: 1 | (2 4 over 3 … ) — 2 + 1, then 2 + 2, then 3 + 2.
        panes.focus(0);
        add(&mut panes, 4);
        assert_eq!(numbers_of(&panes), vec![1, 2, 3, 4]);
        let (areas, _) = panes.areas(Rect::new(0, 0, 201, 40));
        assert_eq!(
            (areas[1], areas[2], areas[3]),
            (
                Rect::new(101, 0, 49, 20),
                Rect::new(151, 0, 50, 20),
                Rect::new(101, 20, 100, 20)
            )
        );
        add(&mut panes, 5);
        add(&mut panes, 6);
        add(&mut panes, 7);
        assert_eq!(panes.visible(), MOST);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(panes.building.is_none(), "at most seven");
        // Closing pane 2: the others move up in reading order.
        panes.focus(1);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(numbers_of(&panes), vec![1, 3, 4, 5, 6, 7]);
        assert_eq!(panes.grid.others, [3, 4, 5, 6, 7]);
        // The main pane is never closed; /swap puts another in its place, the keys staying where they are.
        panes.focus(0);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(panes.visible(), 6, "the main pane stays");
        panes.active().pane_request = Some(Request::Swap(Some("4".into())));
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert_eq!(numbers_of(&panes), vec![4, 3, 1, 5, 6, 7]);
        assert_eq!(panes.list[panes.active].number, 1, "the keys stay");
    }

    #[tokio::test]
    async fn a_new_pane_waits_for_room_and_the_border_moves_by_key_and_mouse() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = split();
        // A window just wide enough for two: a third would be under 40 columns or 8 rows nowhere — two over each other
        // fit, but a narrow low window refuses it.
        let _ = panes.areas(Rect::new(0, 0, 90, 14));
        assert!(!panes.room_for_one_more(), "7 rows each");
        let _ = panes.areas(Rect::new(0, 0, 90, 40));
        assert!(panes.room_for_one_more());
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('}'), KeyModifiers::NONE);
        assert_eq!(panes.grid.share, SHARE + 5);
        for _ in 0..20 {
            panes.active().pane_request = Some(Request::Widen(-SHARE_STEP));
            panes.settle(&host);
        }
        assert_eq!(panes.grid.share, SHARE_LEAST, "within its bounds");
        // The border dragged by the mouse.
        let _ = panes.areas(Rect::new(0, 0, 100, 40));
        let border = panes.placed.border.unwrap();
        let mouse = |kind, column| {
            TermEvent::Mouse(MouseEvent {
                kind,
                column,
                row: 5,
                modifiers: KeyModifiers::NONE,
            })
        };
        panes.on_terminal(mouse(MouseEventKind::Down(MouseButton::Left), border.x));
        panes.on_terminal(mouse(MouseEventKind::Drag(MouseButton::Left), 70));
        panes.on_terminal(mouse(MouseEventKind::Up(MouseButton::Left), 70));
        assert_eq!((panes.grid.share, panes.dragging), (70, false));
    }

    #[tokio::test]
    async fn ctrl_bracket_moves_the_keys_and_a_click_moves_into_the_pane_under_it() {
        let (mut panes, _inlets) = split();
        press(&mut panes, KeyCode::Char(']'), KeyModifiers::CONTROL);
        assert_eq!(panes.active, 0);
        press(&mut panes, KeyCode::Char('5'), KeyModifiers::CONTROL);
        assert_eq!(panes.active, 1, "the legacy encoding too");
        press(&mut panes, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(panes.active, 0, "⌃P on any layout");
        press(
            &mut panes,
            KeyCode::Char('6'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(panes.active, 1, "⌃] on a German layout: ⌃⌥6");
        press(
            &mut panes,
            KeyCode::Char('9'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(panes.active, 0, "⌃] from the owner's Voyager: ⌃⌥9");
        press(&mut panes, KeyCode::Char('9'), KeyModifiers::CONTROL);
        assert_eq!(panes.active, 0, "⌃9 alone is no bracket");
        press(&mut panes, KeyCode::Char('p'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('6'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('p'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            panes.list[1].app.input.text(),
            "x",
            "keys go to the active pane alone"
        );
        assert!(panes.list[0].app.input.is_empty());
        panes.list[0].area = Rect::new(0, 0, 40, 20);
        panes.list[1].area = Rect::new(41, 0, 40, 20);
        panes.on_terminal(TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(panes.active, 0);
    }

    #[tokio::test]
    async fn quit_ends_all_but_asks_once_more_while_another_pane_works_and_close_keeps_the_numbers()
    {
        let (mut panes, _inlets) = split();
        panes.list[0].app.working_for_tests();
        panes.list[1].app.quit = true;
        assert!(!panes.quitting(), "pane 1 still works");
        assert!(!panes.list[1].app.quit);
        panes.list[1].app.quit = true;
        assert!(panes.quitting(), "the second /quit ends all");
        let (mut panes, _inlets) = split();
        panes.active().pane_request = Some(Request::Swap(None));
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        panes.active = 1;
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(numbers_of(&panes), vec![2], "pane 2 keeps its number");
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(numbers_of(&panes), vec![2], "the last pane stays");
    }

    #[tokio::test]
    async fn a_switch_goes_at_once_while_nothing_works_and_asks_first_while_a_pane_does() {
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        let there = std::path::PathBuf::from("/elsewhere");
        let (mut panes, _inlets) = split();
        // The window's own folder — asked from any of its panes — is no switch.
        let own = panes.cwd.clone();
        panes.list[1].app.pane_request = Some(Request::Switch(Switch::to(own)));
        panes.settle(&host);
        assert!(panes.switch_to.is_none());
        panes.list[1].app.pane_request = Some(Request::Switch(Switch::to(there.clone())));
        panes.settle(&host);
        assert_eq!(panes.switch_to, Some(Switch::to(there.clone())));
        assert!(panes.quitting(), "the window ends as at a /quit");
        let (mut panes, _inlets) = split();
        panes.list[0].app.working_for_tests();
        panes.list[1].app.pane_request = Some(Request::Switch(Switch::to(there.clone())));
        panes.settle(&host);
        assert!(panes.switch_to.is_none() && !panes.quitting());
        let picker = panes.active().picker.as_ref().unwrap();
        assert!(
            picker.guarded && picker.title.contains("still at work"),
            "{}",
            picker.title
        );
        assert_eq!(
            picker.chosen(),
            Some(crate::picker::Pick::Shown),
            "no is the default"
        );
        panes.active().pane_request = Some(Request::SwitchNow(Switch::to(there.clone())));
        panes.settle(&host);
        assert_eq!(panes.switch_to, Some(Switch::to(there)));
    }

    fn ended(reason: glut::FinishReason) -> Report {
        Report::Done(
            glut::RunOutcome {
                reason,
                answer: None,
                turns: 1,
                usage: glut::Usage::default(),
                refusals: 0,
                warm: None,
                prepared: None,
            },
            Vec::new(),
        )
    }

    #[tokio::test]
    async fn tiles_tell_of_a_background_agent_and_a_subagent_and_a_click_opens_the_agents_tab() {
        let _serial = crate::tiles::serial();
        let (mut panes, _inlets) = with_background("critic");
        let front = panes.list[0].id;
        let behind = panes.list[1].id;
        let mut answered = ended(glut::FinishReason::Answered);
        if let Report::Done(outcome, _) = &mut answered {
            outcome.answer = Some("Found two bugs.\nmore".into());
        }
        panes.on_report(behind, answered);
        assert_eq!(panes.tiles.list.len(), 1);
        let tile = &panes.tiles.list[0];
        assert_eq!(
            (
                tile.who.as_str(),
                tile.stand,
                tile.lines.clone(),
                tile.goes.clone()
            ),
            (
                "critic",
                crate::tiles::Stand::Done,
                vec!["Found two bugs.".to_owned()],
                crate::tiles::Goes::Behind {
                    name: "critic".into(),
                    asks: false
                }
            )
        );
        // A question of the agent stays while it waits.
        let (answer, _answered) = oneshot::channel();
        let request = glut::permissions::AskRequest {
            tool: "bash".into(),
            target: "rm -rf target".into(),
            scopes: Vec::new(),
            from: None,
            keychain: false,
            note: None,
        };
        panes.on_report(behind, Report::Ask(request, answer));
        panes.sync();
        assert!(
            panes
                .tiles
                .list
                .iter()
                .any(|tile| tile.asks_of == Some(behind)
                    && tile.stand == crate::tiles::Stand::Asks
                    && tile.lines == ["rm -rf target"])
        );
        // A subagent of the pane in front that ended.
        let task = |id: &str| glut::Event::ToolCall {
            turn: 1,
            id: id.into(),
            name: "task".into(),
            arguments: r#"{"description":"review a.rs","prompt":"go"}"#.into(),
        };
        let result = |id: &str| glut::Event::ToolResult {
            turn: 1,
            id: id.into(),
            name: "task".into(),
            ok: true,
            output: "Looks fine.".into(),
            truncated: false,
        };
        panes.on_report(front, Report::Event(task("t1")));
        panes.on_report(front, Report::Event(result("t1")));
        let sub = panes
            .tiles
            .list
            .iter()
            .find(|tile| tile.subagents == 1)
            .expect("a subagent's tile");
        assert_eq!(
            (sub.who.as_str(), sub.lines.clone()),
            ("review a.rs", vec!["Looks fine.".to_owned()])
        );
        assert_eq!(panes.tiles.list.len(), 3);
        // A click on it opens the Agents tab at that subagent.
        panes.tiles.place(Rect::new(0, 0, 120, 40), &[]);
        let spot = panes
            .tiles
            .list
            .iter()
            .find(|tile| tile.subagents == 1)
            .unwrap()
            .area;
        panes.on_terminal(TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: spot.x + 2,
            row: spot.y + 1,
            modifiers: KeyModifiers::NONE,
        }));
        let app = &panes.list[0].app;
        assert_eq!((app.panel, app.tab), (Some(true), crate::app::Tab::Agents));
        assert!(app.agents_list.is_some(), "the list takes the keys at it");
        assert_eq!(panes.tiles.list.len(), 2, "the tile clicked went");
        // With the Agents tab shown, what it lists needs no tile.
        panes.list[0].app.panel_drawn = true;
        panes.on_report(front, Report::Event(task("t2")));
        panes.on_report(front, Report::Event(result("t2")));
        assert_eq!(panes.tiles.list.len(), 2);
        // A click on the question's tile brings its agent forward, where its ask card is answered.
        panes.tiles.place(Rect::new(0, 0, 120, 40), &[]);
        let spot = panes
            .tiles
            .list
            .iter()
            .find(|tile| tile.asks_of == Some(behind))
            .unwrap()
            .area;
        panes.on_terminal(TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: spot.x + 2,
            row: spot.y + 1,
            modifiers: KeyModifiers::NONE,
        }));
        let at = panes.at(behind);
        assert!(!panes.list[at].background, "brought forward");
        assert_eq!(panes.active, at, "with the keys");
    }

    #[tokio::test]
    async fn another_pane_ending_is_a_toast_while_the_owner_looks_and_a_notification_while_away() {
        glut::lang::set_for_thread(Some(glut::lang::Lang::En));
        let (mut panes, _inlets) = split();
        panes.active = 0;
        let other = panes.list[1].id;
        let long_ago = Instant::now() - Duration::from_secs(20);
        panes.notifier.focused = Some(true);
        panes.list[1].app.working_for_tests();
        panes.list[1].app.busy_since = Some(long_ago);
        panes.on_report(other, ended(glut::FinishReason::Answered));
        assert_eq!(panes.list[0].app.toast(), Some("pane 2: done after 20 s"));
        assert_eq!(
            panes.notifier.due(),
            None,
            "no message while the owner looks"
        );
        // The owner stopped it: nothing to tell.
        panes.notifier.focused = Some(false);
        panes.list[1].app.working_for_tests();
        panes.list[1].app.busy_since = Some(long_ago);
        panes.on_report(other, ended(glut::FinishReason::Cancelled));
        assert_eq!(panes.notifier.due(), None);
        // A short turn's end waits for no message either; a long one's does.
        panes.list[1].app.working_for_tests();
        panes.list[1].app.busy_since = Some(Instant::now());
        panes.on_report(other, ended(glut::FinishReason::Answered));
        assert_eq!(panes.notifier.due(), None);
        panes.list[1].app.working_for_tests();
        panes.list[1].app.busy_since = Some(long_ago);
        panes.on_report(other, ended(glut::FinishReason::Failed));
        assert!(panes.notifier.due().is_some());
        let mut out = Vec::new();
        panes.send_notifications(&mut out);
        let sent = String::from_utf8(out).unwrap();
        assert!(sent.ends_with('\x07'), "the bell: {sent:?}");
        assert_eq!(panes.notifier.due(), None, "sent");
        // A focus report from the terminal is the notifier's, no pane's key.
        panes.on_terminal(TermEvent::FocusGained);
        assert_eq!(panes.notifier.focused, Some(true));
        glut::lang::set_for_thread(None);
    }

    #[tokio::test]
    async fn a_kill_ends_every_pane_even_while_one_works() {
        let (mut panes, _inlets) = split();
        panes.list[0].app.working_for_tests();
        panes.list[1].app.abandon();
        assert!(panes.quitting());
        assert!(panes.list.iter().all(|pane| pane.app.abandoned));
    }

    #[tokio::test]
    async fn a_closed_sessions_late_report_reaches_no_pane_though_its_number_is_taken_again() {
        let (mut panes, _inlets) = split();
        let old = panes.list[1].id;
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 7,
            number: 2,
            resume: false,
            tree: None,
            role: None,
            background: None,
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        assert_eq!(numbers_of(&panes), vec![1, 2], "the number 2 again");
        let blocks = panes.list[1].app.transcript.blocks.len();
        panes.on_report(old, Report::Host(crate::host::Signal::Note("stale".into())));
        assert_eq!(
            panes.list[1].app.transcript.blocks.len(),
            blocks,
            "the old session's report went nowhere"
        );
    }

    #[tokio::test]
    async fn a_new_pane_leaves_the_keys_where_the_owner_is_typing() {
        let (first, reports) = app();
        let (mut panes, _inlets) = Panes::new(first, reports, true, Servers::Owner);
        panes.active().input.insert_text("half a thought");
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 2,
            number: 2,
            resume: false,
            tree: None,
            role: None,
            background: None,
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        assert_eq!(
            (numbers_of(&panes), panes.active),
            (vec![1, 2], 0),
            "pane 1 keeps the keys"
        );
    }

    #[test]
    fn split_reads_resume_where_and_a_role_but_no_side() {
        assert_eq!(
            parse_split("").unwrap(),
            Request::Split {
                resume: false,
                place: Place::Ask,
                role: None,
            }
        );
        assert_eq!(
            parse_split("resume here").unwrap(),
            Request::Split {
                resume: true,
                place: Place::Here,
                role: None,
            }
        );
        assert_eq!(
            parse_split("tree glut/plan").unwrap(),
            Request::Split {
                resume: false,
                place: Place::Tree("glut/plan".into()),
                role: None,
            }
        );
        assert_eq!(
            parse_split("tree").unwrap(),
            Request::Split {
                resume: false,
                place: Place::Tree(String::new()),
                role: None,
            }
        );
        assert_eq!(
            parse_split("as reviewer tree").unwrap(),
            Request::Split {
                resume: false,
                place: Place::Tree(String::new()),
                role: Some("reviewer".into()),
            }
        );
        assert!(parse_split("as").is_err());
        assert!(parse_split("sideways").is_err());
        let side = parse_split("d").unwrap_err();
        assert!(side.contains("no side"), "{side}");
    }

    #[tokio::test]
    async fn in_a_repository_split_asks_where_and_a_worktree_goes_into_the_input_to_rename() {
        let (_home, repo) = crate::worktree::testing::repo("panes-ask");
        let (mut parts, engine) = session::for_tests();
        parts.config.cwd = repo.clone();
        let (send, reports) = mpsc::unbounded_channel();
        let (mut panes, _inlets) = Panes::new(
            App::new(Arc::new(parts), engine, send),
            reports,
            true,
            Servers::Owner,
        );
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('3'), KeyModifiers::NONE);
        let picker = panes.active().picker.as_ref().expect("asked where");
        assert_eq!(picker.title, "Pane 2: where does it work?");
        assert!(panes.building.is_none(), "nothing starts before the answer");
        press(&mut panes, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(
            panes.active().picker.as_ref().unwrap().selected,
            0,
            "no key before it showed"
        );
        press(&mut panes, KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(
            panes.active().picker.as_ref().unwrap().query.is_empty(),
            "typing does not filter it"
        );
        if let Some(picker) = &mut panes.active().picker {
            picker.opened = Instant::now() - Duration::from_secs(1);
        }
        press(&mut panes, KeyCode::Down, KeyModifiers::NONE);
        press(&mut panes, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(panes.active().input.text(), "/split tree glut/pane-2");
        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    }

    #[tokio::test]
    async fn a_worktrees_pane_ends_as_picked_and_discarding_work_is_asked_twice() {
        let (home, repo) = crate::worktree::testing::repo("panes-leave");
        let repo = crate::worktree::repo_of(&repo).unwrap().unwrap();
        let tree = crate::worktree::add(&home, &repo, "glut/pane-2").unwrap();
        crate::worktree::testing::commit(&tree.place, "b.txt", "two\n");
        let (mut panes, _inlets) = split();
        panes.list[1].tree = Some(tree.clone());
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(
            panes
                .active()
                .picker
                .as_ref()
                .map(|picker| picker.title.clone()),
            Some("Closing this worktree's pane".to_owned())
        );
        panes.active().picker = None;
        panes.active().pane_request = Some(Request::CloseAs(Leaving::Discard));
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert_eq!(
            panes
                .active()
                .picker
                .as_ref()
                .map(|picker| picker.title.clone()),
            Some("Discard glut/pane-2?".to_owned()),
            "a commit would be lost: asked again"
        );
        assert!(tree.place.exists() && panes.list.len() == 2);
        panes.active().picker = None;
        panes.active().pane_request = Some(Request::CloseAs(Leaving::DiscardSure));
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert!(!tree.place.exists() && panes.list.len() == 1);
        let _ = std::fs::remove_dir_all(repo.top.parent().unwrap());
    }

    #[tokio::test]
    async fn a_model_without_room_on_the_screen_waits_and_is_never_counted_as_failed() {
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        let (mut panes, _inlets) = split();
        // Far too narrow for a second pane beside the main one.
        panes.room = ratatui::layout::Rect::new(0, 0, 20, 40);
        let asked = crate::arena::Asked {
            contenders: vec![crate::arena::Contender {
                provider: "p".into(),
                model: "m".into(),
                local: false,
            }],
            task: "fix it".into(),
        };
        panes.arena = Some(crate::arena::Arena::new(asked, panes.list[0].id));
        panes.settle(&host);
        panes.settle(&host);
        let arena = panes.arena.as_ref().unwrap();
        assert_eq!(arena.members[0].state, crate::arena::State::Waiting);
        let told = panes.list[0]
            .app
            .transcript
            .blocks
            .iter()
            .filter(|block| {
                matches!(&block.kind, crate::transcript::Kind::Note { text, .. } if text.contains("waits for room"))
            })
            .count();
        assert_eq!(told, 1, "said once, not at every turn of the loop");
    }

    #[tokio::test]
    async fn an_arena_gives_cloud_models_the_task_first_stops_one_past_its_cost_and_compares_at_the_end()
     {
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        let owners = |panes: &Panes, id: u64| -> Vec<String> {
            let pane = panes.list.iter().find(|pane| pane.id == id).unwrap();
            pane.app
                .transcript
                .blocks
                .iter()
                .filter_map(|block| match &block.kind {
                    crate::transcript::Kind::Owner(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        let (mut panes, _inlets) = split();
        add(&mut panes, 3);
        let contender = |local: bool| crate::arena::Contender {
            provider: "p".into(),
            model: "m".into(),
            local,
        };
        let asked = crate::arena::Asked {
            contenders: vec![contender(true), contender(false)],
            task: "fix it".into(),
        };
        let mut arena = crate::arena::Arena::new(asked, panes.list[0].id);
        for (member, id) in arena.members.iter_mut().zip([2, 3]) {
            member.state = crate::arena::State::Ready;
            member.pane = Some(id);
        }
        panes.arena = Some(arena);
        // A list open in a model's pane holds its task back: its clock has not started.
        let at = panes.at(3);
        panes.list[at].app.picker = Some(crate::picker::Picker::new("x", Vec::new(), None));
        panes.settle(&host);
        assert!(owners(&panes, 3).is_empty());
        let at = panes.at(3);
        panes.list[at].app.picker = None;
        panes.settle(&host);
        assert!(owners(&panes, 2).is_empty(), "the local model waits");
        assert_eq!(owners(&panes, 3), ["fix it"]);
        let state =
            |panes: &Panes, at: usize| panes.arena.as_ref().unwrap().members[at].state.clone();
        assert_eq!(state(&panes, 1), crate::arena::State::Working);
        let at = panes.at(3);
        panes.list[at].app.cost = Some(crate::arena::COST_LIMIT + 0.5);
        panes.settle(&host);
        assert!(matches!(state(&panes, 1), crate::arena::State::Aborted(_)));
        assert_eq!(owners(&panes, 2), ["fix it"], "now the local one goes");
        // A late end of the stopped one keeps it aborted, its refusals counted.
        let mut late = ended(glut::FinishReason::Cancelled);
        if let Report::Done(outcome, _) = &mut late {
            outcome.refusals = 2;
        }
        panes.on_report(3, late);
        assert!(matches!(state(&panes, 1), crate::arena::State::Aborted(_)));
        assert_eq!(panes.arena.as_ref().unwrap().members[1].refusals, 2);
        // Its pane closes only with the arena.
        panes.active = panes.at(2);
        panes.active().pane_request = Some(Request::Close);
        panes.settle(&host);
        assert_eq!(panes.list.len(), 3);
        // A turn the host refused never ends with a `Done`: the model is aborted.
        panes.on_report(
            2,
            Report::Refused {
                reason: "a hook said no".into(),
                prompt: "fix it".into(),
                conversation: Vec::new(),
            },
        );
        assert!(matches!(state(&panes, 0), crate::arena::State::Aborted(_)));
        panes.settle(&host);
        assert!(panes.arena.as_ref().unwrap().compared);
        let notes: Vec<String> = panes.list[0]
            .app
            .transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Note { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            notes.iter().any(|note| note.starts_with("Arena — fix it")),
            "{notes:?}"
        );
        // The card comes on the owner's word only; a check's end is counted for the check last started there.
        assert!(panes.card.is_none());
        panes.active().pane_request = Some(Request::ArenaCard);
        panes.settle(&host);
        assert!(panes.card.is_some());
        panes.card.as_mut().unwrap().drawn = true;
        let stale = Report::Shell {
            id: format!("{}0", crate::app::ARENA_CHECK),
            command: "cargo test".into(),
            ran: crate::shell::Ran {
                ok: false,
                output: "old".into(),
            },
            to_model: false,
        };
        panes.arena.as_mut().unwrap().check_ids[1] = Some(format!("{}1", crate::app::ARENA_CHECK));
        panes.on_report(3, stale);
        assert_eq!(
            panes.arena.as_ref().unwrap().checks[1],
            crate::arena::Check::None
        );
        panes.on_report(
            3,
            Report::Shell {
                id: format!("{}1", crate::app::ARENA_CHECK),
                command: "cargo test".into(),
                ran: crate::shell::Ran {
                    ok: true,
                    output: "ok".into(),
                },
                to_model: false,
            },
        );
        assert_eq!(
            panes.arena.as_ref().unwrap().checks,
            [crate::arena::Check::None, crate::arena::Check::Passed]
        );
        // The card holds the keys: Esc closes it, and nothing reached the pane behind.
        let key = |code| {
            TermEvent::Key(crossterm::event::KeyEvent::new(
                code,
                crossterm::event::KeyModifiers::NONE,
            ))
        };
        let before = panes.active().input.text();
        panes.on_terminal(key(KeyCode::Char('x')));
        assert_eq!(panes.active().input.text(), before);
        panes.on_terminal(key(KeyCode::Esc));
        assert!(panes.card.is_none());
        // Ended: asked first, then the panes go.
        panes.active = 0;
        panes.active().pane_request = Some(Request::ArenaEnd(false));
        panes.settle(&host);
        assert!(
            panes
                .active()
                .picker
                .as_ref()
                .is_some_and(|picker| picker.guarded)
        );
        panes.active().picker = None;
        panes.active().pane_request = Some(Request::ArenaEnd(true));
        panes.settle(&host);
        assert!(panes.arena.is_none());
        assert_eq!(panes.list.len(), 1);
    }

    #[tokio::test]
    async fn a_bench_task_keeps_each_models_result_once_its_checks_are_over_then_the_set_ends() {
        let host: Arc<dyn Host> = Arc::new(crate::Standalone);
        let (mut panes, _inlets) = split();
        add(&mut panes, 3);
        let home = panes.list[0].app.parts.home.clone();
        let contender = |model: &str| crate::arena::Contender {
            provider: "p".into(),
            model: model.into(),
            local: false,
        };
        let asked = crate::arena::Asked {
            contenders: vec![contender("good"), contender("bad")],
            task: "fix it".into(),
        };
        let mut arena = crate::arena::Arena::new(asked, panes.list[0].id);
        for (member, id) in arena.members.iter_mut().zip([2, 3]) {
            member.state = crate::arena::State::Done(glut::FinishReason::Answered);
            member.pane = Some(id);
            member.took = Some(Duration::from_secs(9));
        }
        arena.compared = true;
        arena.checking = true;
        arena.checks = vec![crate::arena::Check::Passed, crate::arena::Check::Running];
        panes.arena = Some(arena);
        let place =
            crate::bench::bench_dir().join(format!("2026-11-t-test-{}", std::process::id()));
        std::fs::create_dir_all(&place).unwrap();
        let task = crate::bench::Task {
            name: "t".into(),
            text: "fix it".into(),
            time_limit: crate::arena::TIME_LIMIT,
            cost_limit: crate::arena::COST_LIMIT,
            folder: home.join("glut-arena/sets/2026-11/t"),
        };
        panes.bench = Some(crate::bench::Bench {
            home: home.clone(),
            home_pane: panes.list[0].id,
            set: "2026-11".into(),
            contenders: Vec::new(),
            left: std::collections::VecDeque::new(),
            current: Some((task, place.clone())),
            total: 1,
            kept: Vec::new(),
        });
        panes.settle(&host);
        assert!(
            crate::bench::results(&home).is_empty(),
            "a check still runs"
        );
        panes.arena.as_mut().unwrap().checks[1] = crate::arena::Check::Failed;
        panes.settle(&host);
        let passed: Vec<(String, bool)> = crate::bench::results(&home)
            .into_iter()
            .map(|record| (record.model, record.passed))
            .collect();
        assert_eq!(
            passed,
            [("p/good".to_owned(), true), ("p/bad".to_owned(), false)]
        );
        assert!(
            panes.arena.is_none() && panes.bench.is_none(),
            "the set is through"
        );
        assert_eq!(panes.list.len(), 1, "the task's panes went");
        assert!(!place.exists(), "its start repository too");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn send_reads_a_pane_number_then_the_text_or_plan() {
        assert_eq!(
            parse_send("hello").unwrap(),
            Request::Send {
                to: None,
                text: "hello".into(),
                whole: "hello".into()
            }
        );
        assert_eq!(
            parse_send("look at a.rs").unwrap(),
            // The first word may name an agent; the screen decides when it settles.
            Request::Send {
                to: Some("look".to_owned()),
                text: "at a.rs".into(),
                whole: "look at a.rs".into()
            }
        );
        assert_eq!(
            parse_send("2 look").unwrap(),
            Request::Send {
                to: Some("2".to_owned()),
                text: "look".into(),
                whole: "2 look".into()
            }
        );
        assert_eq!(parse_send("plan").unwrap(), Request::SendPlan { to: None });
        assert_eq!(
            parse_send("2 plan").unwrap(),
            Request::SendPlan {
                to: Some("2".to_owned())
            }
        );
        assert_eq!(parse_send("").unwrap(), Request::SendPick);
        assert!(parse_send("2").is_err());
    }

    #[tokio::test]
    async fn a_message_reaches_the_other_pane_as_if_typed_there_now_or_after_its_turn() {
        let owners = |app: &App| -> Vec<String> {
            app.transcript
                .blocks
                .iter()
                .filter_map(|block| match &block.kind {
                    crate::transcript::Kind::Owner(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        let (mut panes, _inlets) = split();
        panes.active = 0;
        panes.sync();
        panes.active().pane_request = Some(parse_send("look at a.rs").unwrap());
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert_eq!(owners(&panes.list[1].app), ["From pane 1: look at a.rs"]);
        assert_eq!(panes.active, 0, "the keys stay");
        panes.list[1].app.working_for_tests();
        panes.active().pane_request = Some(parse_send("2 and the tests").unwrap());
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert_eq!(owners(&panes.list[1].app).len(), 1, "it waits for the turn");
        panes.active().pane_request = Some(Request::SendPlan { to: None });
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        let notes: Vec<String> = panes.list[0]
            .app
            .transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Note { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            notes.iter().any(|note| note.contains("after its turn")),
            "{notes:?}"
        );
        assert!(
            notes.iter().any(|note| note.contains("no plan yet")),
            "{notes:?}"
        );
        // A first number naming no other pane is the message's own word.
        let (mut panes, _inlets) = split();
        panes.active = 0;
        panes.active().pane_request = Some(parse_send("3 files changed").unwrap());
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert_eq!(owners(&panes.list[1].app), ["From pane 1: 3 files changed"]);
    }

    /// What this window answers another one (T83).
    fn asked(panes: &mut Panes, ask: crate::sessions::Ask) -> crate::sessions::Reply {
        let (reply, mut answer) = oneshot::channel();
        panes.on_incoming(crate::sessions::Incoming { ask, reply });
        answer.try_recv().unwrap()
    }

    fn letter(by: crate::sessions::By, mode: &str, to: &str) -> crate::sessions::Letter {
        crate::sessions::Letter {
            from: "beta-xy/2".into(),
            to: to.into(),
            text: "which test fails?".into(),
            by,
            mode: mode.into(),
            notify: false,
            chain: 0,
            rights: None,
        }
    }

    #[tokio::test]
    async fn another_window_learns_the_panes_and_its_messages_come_in_by_the_mode() {
        use crate::sessions::{Ask, By};
        let (mut panes, _inlets) = split();
        panes.sync();
        let card = asked(&mut panes, Ask::Who).card.unwrap();
        assert_eq!(card.seats.len(), 2);
        assert!(card.seats[1].address.ends_with("/2"), "{:?}", card.seats);
        // Q6b: a pane that can do no more than the sender, in its folder and mode, starts on it; one in another folder,
        // or a letter without the sender's rights, waits for the owner — as `tell` between panes of one window.
        let (mode, rights) = {
            let target = &panes.list[1].app;
            (
                target.mode().name().to_owned(),
                crate::sessions::Rights {
                    sender: target.sender(),
                    folder: target.parts.config.cwd.clone(),
                },
            )
        };
        let mut elsewhere = letter(By::Model, &mode, "2");
        elsewhere.rights = Some(crate::sessions::Rights {
            folder: "/elsewhere".into(),
            ..rights.clone()
        });
        let sent = asked(&mut panes, Ask::Send(elsewhere))
            .sent
            .unwrap()
            .unwrap();
        assert!(sent.starts_with("waits for approval"), "{sent}");
        panes.list[1].app.held.clear();
        let unknown = letter(By::Model, &mode, "2");
        let sent = asked(&mut panes, Ask::Send(unknown)).sent.unwrap().unwrap();
        assert!(sent.starts_with("waits for approval"), "{sent}");
        panes.list[1].app.held.clear();
        let mut alike = letter(By::Model, &mode, "2");
        alike.rights = Some(rights);
        let sent = asked(&mut panes, Ask::Send(alike)).sent.unwrap().unwrap();
        assert!(sent.starts_with("delivered"), "{sent}");
        // From another mode a model's message waits for the owner there (Q6), shown as no word of the owner's.
        let sent = asked(&mut panes, Ask::Send(letter(By::Model, "auto", "2")))
            .sent
            .unwrap();
        assert!(sent.unwrap().starts_with("waits for approval"));
        let target = &panes.list[1].app;
        assert_eq!(target.held.len(), 1);
        assert!(target.transcript.blocks.iter().any(|block| matches!(
            &block.kind,
            crate::transcript::Kind::Note { text, .. }
                if text.starts_with("beta-xy/2's model (not the owner): which test fails?")
        )));
        // The owner's own word from there goes in at once, whatever the mode.
        let sent = asked(&mut panes, Ask::Send(letter(By::Owner, "auto", "")))
            .sent
            .unwrap();
        assert!(sent.is_ok(), "{sent:?}");
        // A pane it does not have, and a window that takes nothing from outside.
        assert!(
            asked(&mut panes, Ask::Send(letter(By::Model, "default", "7")))
                .sent
                .unwrap()
                .is_err()
        );
        panes.sessions_off = true;
        assert!(
            asked(&mut panes, Ask::Send(letter(By::Owner, "default", "")))
                .sent
                .unwrap()
                .is_err()
        );
        assert!(asked(&mut panes, Ask::Who).card.unwrap().off);
    }

    fn told(panes: &mut Panes, from: u64, to: Option<&str>, text: &str) -> Result<String, String> {
        let (reply, mut answer) = oneshot::channel();
        panes.on_report(
            from,
            Report::Tell {
                to: to.map(str::to_owned),
                text: text.into(),
                notify: false,
                reply,
            },
        );
        answer.try_recv().unwrap()
    }

    #[tokio::test]
    async fn a_models_message_is_no_word_of_the_owners_and_a_long_chain_waits_for_one() {
        let (mut panes, _inlets) = split();
        panes.sync();
        let (first, second) = (panes.list[0].id, panes.list[1].id);
        let said = told(&mut panes, first, None, "which test fails?").unwrap();
        assert!(said.contains("delivered: pane 2"), "{said}");
        let target = &panes.list[1].app;
        assert!(target.transcript.blocks.iter().any(|block| matches!(
            &block.kind,
            crate::transcript::Kind::Note { text, tone: Tone::Pane(1) }
                if text.starts_with("pane 1's model (not the owner): which test fails?")
        )));
        assert!(
            !target
                .transcript
                .blocks
                .iter()
                .any(|block| matches!(block.kind, crate::transcript::Kind::Owner(_))),
            "never shown as the owner's"
        );
        assert!(
            !panes.list[1].app.owner_spoke,
            "nor counted as the owner's word"
        );
        for n in 2..=MOST_CHAIN {
            let from = if n % 2 == 0 { second } else { first };
            assert!(
                told(&mut panes, from, None, "and?")
                    .unwrap()
                    .starts_with("delivered")
            );
        }
        // Past the chain the message waits for Enter — also in a pane whose turn ends meanwhile.
        let held = told(&mut panes, second, None, "one more").unwrap();
        assert!(held.starts_with("held"), "{held}");
        assert_eq!(panes.list[0].app.held.len(), 1);
        assert!(
            panes.list[0]
                .app
                .queue
                .iter()
                .all(|queued| !queued.contains("one more")),
            "a held message is not what a turn's end sends"
        );
        // The owner's word anywhere starts the count anew.
        panes.list[0].app.owner_spoke = true;
        panes.sync();
        assert!(
            told(&mut panes, second, None, "again")
                .unwrap()
                .starts_with("delivered"),
            "the chain starts anew"
        );
        panes.list[1].app.tells_sent = MOST_TELLS;
        assert!(
            told(&mut panes, second, None, "too many")
                .unwrap_err()
                .contains("no more")
        );
        assert!(
            told(&mut panes, first, Some("9"), "x")
                .unwrap_err()
                .contains("no pane or agent 9")
        );
    }

    #[tokio::test]
    async fn a_model_does_not_start_a_turn_in_a_looser_pane_and_enter_lets_it_go_on() {
        use glut::permissions::{AnswerFuture, AskRequest, Asker, Mode};
        struct No;
        impl Asker for No {
            fn ask<'a>(&'a self, _request: &'a AskRequest) -> AnswerFuture<'a> {
                Box::pin(async { glut::permissions::Answer::Deny })
            }
        }
        let gated = |mode: Mode| {
            let (mut parts, engine) = session::for_tests();
            parts.tools.set_gate(Arc::new(glut::Gate::new(
                glut::RuleSet::default(),
                mode,
                Arc::new(No),
            )));
            let (send, reports) = mpsc::unbounded_channel();
            (App::new(Arc::new(parts), engine, send), reports)
        };
        let (first_app, first_reports) = gated(Mode::Plan);
        let (mut panes, _inlets) = Panes::new(first_app, first_reports, true, Servers::Owner);
        let (second_app, _second_reports) = gated(Mode::Auto);
        panes.list.push(Pane {
            looping: None,
            id: 2,
            number: 2,
            name: None,
            background: false,
            app: second_app,
            area: Rect::default(),
            tree: None,
        });
        panes.sync();
        let first = panes.list[0].id;
        let held = told(&mut panes, first, None, "build it all").unwrap();
        assert!(held.contains("can do more"), "{held}");
        let target = &mut panes.list[1].app;
        assert!(!target.working() && target.held.len() == 1);
        assert!(target.transcript.blocks.iter().any(|block| matches!(
            &block.kind,
            crate::transcript::Kind::Note { text, .. } if text.contains("build it all") && text.contains("held")
        )), "the owner reads it before letting it go");
        panes.active = 1;
        press(&mut panes, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            panes.list[1].app.held.len(),
            1,
            "no Enter right after it came lets it go"
        );
        panes.list[1].app.held_since = Some(Instant::now() - Duration::from_secs(1));
        press(&mut panes, KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            panes.list[1].app.held.is_empty() && panes.list[1].app.working(),
            "Enter lets it go on"
        );
    }

    #[tokio::test]
    async fn a_model_does_not_start_a_turn_in_a_pane_of_another_folder() {
        let (first_app, first_reports) = app();
        let (mut panes, _inlets) = Panes::new(first_app, first_reports, true, Servers::Owner);
        let (mut parts, engine) = session::for_tests();
        parts.config.cwd = std::env::temp_dir().join("glut-tui-elsewhere");
        let (send, _reports) = mpsc::unbounded_channel();
        panes.list.push(Pane {
            looping: None,
            id: 2,
            number: 2,
            name: None,
            background: false,
            app: App::new(Arc::new(parts), engine, send),
            area: Rect::default(),
            tree: None,
        });
        panes.sync();
        let held = told(&mut panes, 1, None, "change main.rs").unwrap();
        assert!(held.contains("its folder"), "{held}");
        assert!(!panes.list[1].app.working() && panes.list[1].app.held.len() == 1);
    }

    #[tokio::test]
    async fn a_pane_plays_a_role_its_tools_narrowed_its_prompt_ready_and_the_others_told() {
        use glut::permissions::{AnswerFuture, AskRequest, Asker, Mode};
        struct Yes;
        impl Asker for Yes {
            fn ask<'a>(&'a self, _request: &'a AskRequest) -> AnswerFuture<'a> {
                Box::pin(async { glut::permissions::Answer::Allow })
            }
        }
        let (mut parts, engine) = session::for_tests();
        let gate = Arc::new(glut::Gate::new(
            glut::RuleSet::default(),
            Mode::Auto,
            Arc::new(Yes),
        ));
        parts.tools.set_gate(Arc::clone(&gate));
        parts.config.agents = Arc::new(glut::agents::Catalog::default().with_glut_own());
        let (send, reports) = mpsc::unbounded_channel();
        let (mut panes, _inlets) = Panes::new(
            App::new(Arc::new(parts), engine, send),
            reports,
            true,
            Servers::Owner,
        );
        let edit = glut::Access::Edit {
            path: "a.rs".into(),
        };
        assert!(gate.judge("edit_file", &edit).await.is_ok());
        panes.active().play(Some("reviewer"));
        assert_eq!(panes.active().role.as_deref(), Some("reviewer"));
        assert!(
            gate.judge("edit_file", &edit).await.is_err(),
            "a reviewer changes nothing"
        );
        let (name, prompt) = panes.active().parts.pane_link.role().unwrap();
        assert!(name == "reviewer" && prompt.contains("You are the reviewer"));
        panes.active().play(Some("nobody"));
        assert_eq!(
            panes.active().role.as_deref(),
            Some("reviewer"),
            "an unknown role changes nothing"
        );
        panes.active().play(None);
        assert!(gate.judge("edit_file", &edit).await.is_ok());
        assert!(panes.active().parts.pane_link.role().is_none());
    }

    #[tokio::test]
    async fn a_reviewer_does_not_drive_an_all_rounder_and_a_role_named_wrong_splits_nothing() {
        use glut::permissions::{AnswerFuture, AskRequest, Asker, Mode};
        struct Yes;
        impl Asker for Yes {
            fn ask<'a>(&'a self, _request: &'a AskRequest) -> AnswerFuture<'a> {
                Box::pin(async { glut::permissions::Answer::Allow })
            }
        }
        let gated = || {
            let (mut parts, engine) = session::for_tests();
            parts.tools.set_gate(Arc::new(glut::Gate::new(
                glut::RuleSet::default(),
                Mode::Auto,
                Arc::new(Yes),
            )));
            parts.config.agents = Arc::new(glut::agents::Catalog::default().with_glut_own());
            let (send, reports) = mpsc::unbounded_channel();
            (App::new(Arc::new(parts), engine, send), reports)
        };
        let (first, first_reports) = gated();
        let (mut panes, _inlets) = Panes::new(first, first_reports, true, Servers::Owner);
        let (second, _second_reports) = gated();
        panes.list.push(Pane {
            looping: None,
            id: 2,
            number: 2,
            name: None,
            background: false,
            app: second,
            area: Rect::default(),
            tree: None,
        });
        panes.active().play(Some("reviewer"));
        panes.sync();
        let held = told(&mut panes, 1, None, "fix it all now").unwrap();
        assert!(held.starts_with("held"), "{held}");
        assert!(!panes.list[1].app.working());
        // Pane 2 telling the reviewer: a reviewer can do less, so it may start on it.
        assert!(
            told(&mut panes, 2, None, "please look")
                .unwrap()
                .starts_with("delivered")
        );
        panes.active = 1;
        panes.active().pane_request = Some(parse_split("as nobody here").unwrap());
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert!(
            panes.building.is_none(),
            "no pane starts for a role that is not there"
        );
    }

    #[test]
    fn bg_reads_a_name_a_role_and_where() {
        assert!(matches!(
            parse_background("").unwrap(),
            Request::Background {
                name: None,
                role: None,
                place: Place::Ask,
                sure: false
            }
        ));
        let Request::Background {
            name, role, place, ..
        } = parse_background("critic as reviewer tree fix-x").unwrap()
        else {
            panic!("not a background request");
        };
        assert_eq!(
            (name.as_deref(), role.as_deref()),
            (Some("critic"), Some("reviewer"))
        );
        assert_eq!(place, Place::Tree("fix-x".to_owned()));
        assert!(matches!(
            parse_background("as reviewer here").unwrap(),
            Request::Background {
                name: None,
                place: Place::Here,
                ..
            }
        ));
        // A number is a pane's, never an agent's name; so is a second name or a role left out.
        assert!(parse_background("2").is_err());
        assert!(parse_background("one two").is_err());
        assert!(parse_background("as").is_err());
        assert!(parse_background("Big").is_err());
    }

    #[tokio::test]
    async fn btw_asks_a_background_agent_by_name_and_a_word_naming_none_is_the_question_s_own() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_background("critic");
        let ask = |panes: &mut Panes, typed: &str| {
            panes.active().input.insert_text(typed);
            press(panes, KeyCode::Enter, KeyModifiers::NONE);
            panes.settle(&host);
        };
        ask(&mut panes, "/btw critic what did you find?");
        let side = panes.list[0]
            .app
            .side
            .as_ref()
            .expect("a card over the asking pane");
        assert_eq!(side.about.as_deref(), Some("critic"));
        assert_eq!(side.question, "what did you find?");
        assert!(
            panes.list[1].app.side.is_none(),
            "nothing shown in the agent asked"
        );
        assert!(
            panes.list[1]
                .app
                .transcript
                .blocks
                .iter()
                .all(|block| { !matches!(&block.kind, crate::transcript::Kind::Owner(_)) }),
            "the agent's stream untouched"
        );
        // A new one replaces it; a first word that names no one is the question's.
        ask(&mut panes, "/btw update the plan?");
        let side = panes.list[0].app.side.as_ref().unwrap();
        assert_eq!(side.about, None);
        assert_eq!(side.question, "update the plan?");
        ask(&mut panes, "/btw");
        assert!(
            panes.list[0].app.side.is_some(),
            "a usage note, the card stays"
        );
    }

    #[tokio::test]
    async fn an_agent_in_the_background_has_no_room_is_reached_by_name_and_comes_forward() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_background("critic");
        assert_eq!(
            (panes.visible(), panes.split(), panes.active),
            (1, false, 0)
        );
        assert!(panes.list[1].background);
        // ⌃P stays: there is no other pane on the screen.
        press(&mut panes, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(panes.active, 0);
        // The owner's message reaches it by name, the Agents tab lists it.
        panes.sync();
        let behind = &panes.list[0].app.background[0];
        assert_eq!(
            (behind.name.as_str(), behind.role.as_str(), behind.doing),
            ("critic", "all-rounder", crate::agents::Doing::Rests)
        );
        panes.active().pane_request = Some(parse_send("critic look at a.rs").unwrap());
        panes.settle(&host);
        let owners: Vec<String> = panes.list[1]
            .app
            .transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Owner(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(owners, ["From pane 1: look at a.rs"]);
        // Peeking shows it over the pane; Esc closes the peek and nothing else.
        panes.active().pane_request = Some(Request::Peek(Some("critic".into())));
        panes.settle(&host);
        assert_eq!(panes.peek, Some(panes.list[1].id));
        press(&mut panes, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(panes.peek, None);
        // Shown, it stands beside the pane with the keys; hidden again, the keys come back.
        panes.active().pane_request = Some(Request::Show("critic".into()));
        panes.settle(&host);
        assert_eq!((panes.visible(), panes.active), (2, 1));
        assert!(!panes.list[1].background);
        panes.active().pane_request = Some(Request::Hide);
        panes.settle(&host);
        assert_eq!((panes.visible(), panes.active), (1, 0));
        assert_eq!(panes.list[1].name.as_deref(), Some("critic"));
        // The last pane on the screen never hides.
        panes.active().pane_request = Some(Request::Hide);
        panes.settle(&host);
        assert_eq!(panes.visible(), 1);
        panes.active().pane_request = Some(Request::Show("nobody".into()));
        panes.settle(&host);
        assert_eq!(panes.visible(), 1);
    }

    #[tokio::test]
    async fn a_seventh_agent_in_the_background_is_asked_for_once() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_background("a1");
        for n in 2..=BACKGROUND_ASKED {
            let (sender, _) = mpsc::unbounded_channel();
            panes.building = Some(Building {
                looping: None,
                id: 10 + n as u64,
                number: 10 + n as u8,
                resume: false,
                tree: None,
                role: None,
                background: Some(format!("a{n}")),
                restore: None,
                reports: sender,
                arena: None,
            });
            panes.on_built(Ok(session::for_tests()));
        }
        assert_eq!(panes.list.len() - panes.visible(), BACKGROUND_ASKED);
        panes.active().pane_request = Some(parse_background("a7 here").unwrap());
        panes.settle(&host);
        assert!(panes.building.is_none(), "nothing starts before the answer");
        let picker = panes.active().picker.as_ref().expect("asked");
        assert!(
            picker.guarded && picker.title.starts_with("7 agents"),
            "{}",
            picker.title
        );
        assert_eq!(
            picker.chosen(),
            Some(crate::picker::Pick::Shown),
            "no is the default"
        );
        let yes = picker
            .shown()
            .iter()
            .find(|item| item.label.starts_with("yes"))
            .map(|item| item.pick.clone());
        assert_eq!(
            yes,
            Some(crate::picker::Pick::Pane(Request::Background {
                name: Some("a7".into()),
                role: None,
                place: Place::Here,
                sure: true,
            }))
        );
    }

    #[tokio::test]
    async fn a_model_tells_an_agent_in_the_background_by_its_name() {
        let (mut panes, _inlets) = with_background("critic");
        panes.sync();
        let (first, agent) = (panes.list[0].id, panes.list[1].id);
        assert!(told(&mut panes, first, Some("critic"), "look at a.rs").is_ok());
        assert!(
            told(&mut panes, agent, None, "done").is_ok(),
            "the only other one"
        );
        assert!(told(&mut panes, first, Some("nobody"), "hi").is_err());
    }

    #[tokio::test]
    async fn closing_a_pane_gives_the_keys_to_a_pane_on_the_screen_never_to_the_background() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_background("critic");
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 3,
            number: 3,
            resume: false,
            tree: None,
            role: None,
            background: Some("tester".to_owned()),
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        panes.active().pane_request = Some(Request::Show("critic".into()));
        panes.settle(&host);
        // pane 1 and critic on the screen, tester behind them; critic holds the keys and closes.
        assert_eq!((panes.visible(), panes.active), (2, 1));
        panes.active().pane_request = Some(Request::Close);
        panes.settle(&host);
        assert_eq!((panes.visible(), panes.list.len()), (1, 2));
        assert!(!panes.list[panes.active].background);
        assert_eq!(panes.list[panes.active].number, 1);
    }

    #[tokio::test]
    async fn a_name_wins_over_a_number_and_a_number_over_a_role_and_two_alike_roles_name_none() {
        let (mut panes, _inlets) = with_background("critic");
        panes.list[0].app.role = Some("critic".into());
        assert_eq!(
            panes.resolve("critic", 9),
            Some(1),
            "the name, not pane 1's role"
        );
        assert_eq!(panes.resolve("1", 9), Some(0));
        panes.list[1].app.role = Some("tester".into());
        panes.list[0].app.role = Some("tester".into());
        assert_eq!(panes.resolve("tester", 9), None, "two play it");
        assert_eq!(panes.resolve_background("tester"), Some(1));
        // "update plan" is a message when "update" names nobody.
        panes.active().pane_request = Some(parse_send("update plan").unwrap());
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        let owners = panes.list[1]
            .app
            .transcript
            .blocks
            .iter()
            .filter(|block| {
                matches!(&block.kind, crate::transcript::Kind::Owner(text) if text.ends_with("update plan"))
            })
            .count();
        assert_eq!(owners, 1);
    }

    /// A screen whose second session runs the loop `/loop <rest>` asks for (T60a).
    fn with_loop(rest: &str) -> (Panes, Inlets) {
        let crate::looping::Command::Start(asked) = crate::looping::parse(rest).unwrap() else {
            panic!("{rest} starts no loop")
        };
        let (first, reports) = app();
        let (mut panes, inlets) = Panes::new(first, reports, true, Servers::Owner);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: Some((asked, Vec::new())),
            id: 2,
            number: 2,
            resume: false,
            tree: None,
            role: None,
            background: Some("loop".to_owned()),
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        (panes, inlets)
    }

    fn notes(app: &App) -> Vec<String> {
        app.transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Note { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_plans_run_that_ended_is_said_at_the_start_and_comes_as_a_tile_while_glut_is_open() {
        use crate::schedule::{Outcome, Record};
        let (first, reports) = app();
        let (mut panes, _inlets) = Panes::new(first, reports, true, Servers::Owner);
        let home = panes.active().parts.home.clone();
        let mut schedules = crate::schedule::Schedules::default();
        schedules.put(crate::schedule::sample("nightly"));
        schedules.save(&home).unwrap();
        let at = |minutes: i64| chrono::Utc::now() + chrono::Duration::minutes(minutes);
        let record = |ended: chrono::DateTime<chrono::Utc>, outcome| Record {
            started: ended - chrono::Duration::minutes(1),
            ended,
            outcome,
            by_hand: false,
            cost: None,
            said: "all green".into(),
            session: None,
            refused: Vec::new(),
            unpriced: false,
        };
        // Glut last looked an hour ago; two runs ended since.
        crate::prefs::set_runs_seen(&home, at(-60)).unwrap();
        crate::schedule::keep_run(&home, "nightly", &record(at(-30), Outcome::Answered)).unwrap();
        crate::schedule::keep_run(&home, "nightly", &record(at(-10), Outcome::Failed)).unwrap();
        panes.watch_runs(at(0));
        let said = notes(&panes.list[0].app);
        assert!(
            said.iter().any(|note| note.contains("nightly 2×")),
            "{said:?}"
        );
        assert!(
            panes.tiles.list.is_empty(),
            "what ran before is said once, not tiled"
        );
        // Nothing new: nothing more.
        panes.watch_runs(at(1));
        assert!(panes.tiles.list.is_empty());
        // A run ends while Glut is open: a tile, going to its plan on the Runs tab.
        std::thread::sleep(std::time::Duration::from_millis(20));
        crate::schedule::keep_run(&home, "nightly", &record(at(2), Outcome::Answered)).unwrap();
        panes.watch_runs(at(3));
        assert_eq!(panes.tiles.list.len(), 1);
        assert_eq!(panes.tiles.list[0].who, "nightly");
        assert_eq!(
            panes.tiles.list[0].goes,
            crate::tiles::Goes::Runs("nightly".into())
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    fn owners_words(app: &App) -> Vec<String> {
        app.transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Owner(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_loops_rounds_follow_each_other_from_their_end_and_a_failed_one_ends_it() {
        let _serial = crate::tiles::serial();
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_loop("1m run the tests");
        let behind = panes.list[1].id;
        assert!(panes.list[1].background && panes.list[1].app.parts.loop_link.on());
        assert!(
            notes(&panes.list[0].app)
                .iter()
                .any(|note| note.contains("loop") && note.contains("run the tests"))
        );
        // The first round starts at once, as the owner's words.
        panes.settle(&host);
        assert!(panes.list[1].looping.as_ref().unwrap().running());
        assert!(panes.list[1].app.working());
        assert_eq!(owners_words(&panes.list[1].app), ["run the tests"]);
        assert_eq!(panes.loop_due(), None, "nothing due while a round runs");
        // It ends answered: no tile, the next round a minute after its end.
        panes.on_report(behind, ended(glut::FinishReason::Answered));
        panes.settle(&host);
        let looping = panes.list[1].looping.as_ref().unwrap();
        assert_eq!(looping.rounds, 1);
        assert!(!looping.running());
        let due = panes.loop_due().unwrap();
        assert!(due > Instant::now() + Duration::from_secs(55));
        assert!(
            panes.tiles.list.is_empty(),
            "a quiet loop's round is no tile"
        );
        panes.sync();
        assert_eq!(
            panes.list[0].app.background[0].looping.as_deref(),
            Some("↻ 1m 1/50")
        );
        // On a subscription the card says its tokens too.
        let looping = panes.list[1].looping.as_mut().unwrap();
        looping.limits.max_tokens = Some(200_000);
        looping.used = 12_000;
        panes.sync();
        let card = panes.list[0].app.background[0].looping.clone().unwrap();
        assert!(
            card.starts_with("↻ 1m 1/50 · ") && card.contains("12k/200k"),
            "{card}"
        );
        // Not before its time; at it, the second round — the conversation goes on.
        panes.step_loops(due - Duration::from_secs(1));
        assert!(!panes.list[1].app.working());
        panes.step_loops(due);
        assert!(panes.list[1].app.working());
        assert_eq!(owners_words(&panes.list[1].app).len(), 2);
        // A round that fails ends the loop: a tile, a line, the agent stays resting and the tool goes.
        panes.on_report(behind, ended(glut::FinishReason::Failed));
        panes.settle(&host);
        assert!(panes.list[1].looping.is_none() && !panes.list[1].app.parts.loop_link.on());
        assert_eq!(panes.list.len(), 2);
        assert_eq!(panes.tiles.list.len(), 1);
        assert_eq!(panes.tiles.list[0].stand, crate::tiles::Stand::Failed);
        assert!(
            notes(&panes.list[0].app)
                .iter()
                .any(|note| note.contains("2"))
        );
        assert_eq!(panes.loop_due(), None);
    }

    #[tokio::test]
    async fn a_loop_keeps_its_panes_role_fails_on_an_idle_task_and_never_spins() {
        let _serial = crate::tiles::serial();
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        // A pane that plays a role starts loops of that role only.
        let (first, reports) = app();
        let (mut panes, _inlets) = Panes::new(first, reports, true, Servers::Owner);
        panes.active().role = Some("reviewer".into());
        let crate::looping::Command::Start(asked) =
            crate::looping::parse("5m as engineer fix it").unwrap()
        else {
            panic!()
        };
        panes.active().pane_request = Some(Request::LoopStart(asked));
        panes.settle(&host);
        assert!(panes.building.is_none());
        assert!(
            notes(&panes.list[0].app)
                .last()
                .unwrap()
                .contains("reviewer")
        );
        let crate::looping::Command::Start(asked) = crate::looping::parse("5m fix it").unwrap()
        else {
            panic!()
        };
        panes.active().pane_request = Some(Request::LoopStart(asked));
        panes.settle(&host);
        // Taken as the loop's role: the test's catalog has none of that name, so it is said there.
        assert!(
            notes(&panes.list[0].app)
                .last()
                .unwrap()
                .contains("reviewer")
        );

        // A task that starts no turn ends the loop at once, failed.
        let (mut panes, _inlets) = with_loop("1m /skill nosuch");
        panes.settle(&host);
        assert!(panes.list[1].looping.is_none());
        assert_eq!(panes.tiles.list.len(), 1);
        assert_eq!(panes.tiles.list[0].stand, crate::tiles::Stand::Failed);

        // A round due while its agent is busy is no wake-up: the report that frees it is.
        let (mut panes, _inlets) = with_loop("1m run the tests");
        panes.list[1].app.working_for_tests();
        panes.settle(&host);
        assert!(panes.list[1].looping.as_ref().unwrap().next.is_some());
        assert_eq!(panes.loop_due(), None);
        // Free again with its time past, it is due at once.
        let (mut panes, _inlets) = with_loop("1m run the tests");
        panes.list[1].looping.as_mut().unwrap().next =
            Some(Instant::now() - Duration::from_secs(5));
        assert!(panes.loop_due().is_some_and(|due| due <= Instant::now()));
    }

    #[tokio::test]
    async fn a_loop_is_listed_stopped_by_its_name_and_its_model_may_end_it() {
        let _serial = crate::tiles::serial();
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_loop("10m fresh notify check the build");
        let behind = panes.list[1].id;
        panes.settle(&host);
        panes.active().pane_request = Some(Request::LoopList);
        panes.settle(&host);
        let listed = notes(&panes.list[0].app).last().cloned().unwrap();
        assert!(
            listed.contains("loop") && listed.contains("10m") && listed.contains("check the build"),
            "{listed}"
        );
        // Its model says the goal is reached: the round ends, and the loop with it.
        panes.list[1]
            .app
            .parts
            .loop_link
            .end_for_tests("the build is green");
        let mut answered = ended(glut::FinishReason::Answered);
        if let Report::Done(outcome, _) = &mut answered {
            outcome.answer = Some("green".into());
        }
        panes.on_report(behind, answered);
        assert_eq!(panes.tiles.list.len(), 1, "notify: the round's own tile");
        panes.settle(&host);
        assert!(panes.list[1].looping.is_none());
        assert_eq!(
            panes.tiles.list.len(),
            1,
            "notify told of the last round: no second tile"
        );

        // `/loop stop` with one loop running ends that one, a name another; the owner's stop is no tile.
        let (mut panes, _inlets) = with_loop("10m check the build");
        panes.settle(&host);
        panes.active().pane_request = Some(Request::LoopStop(Some("nobody".into())));
        panes.settle(&host);
        assert!(panes.list[1].looping.is_some());
        panes.active().pane_request = Some(Request::LoopStop(None));
        panes.settle(&host);
        assert!(panes.list[1].looping.is_none() && panes.list[1].app.stopping());
        assert!(panes.tiles.list.is_empty());
        panes.active().pane_request = Some(Request::LoopStop(None));
        panes.settle(&host);
        assert!(notes(&panes.list[0].app).last().unwrap().contains("/loop"));
    }

    #[tokio::test]
    async fn a_background_agent_ends_by_its_name_and_the_keys_stay() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = with_background("critic");
        panes.active().pane_request = Some(Request::Peek(Some("critic".into())));
        panes.settle(&host);
        panes.peek = None;
        panes.active().pane_request = Some(Request::End("nobody".into()));
        panes.settle(&host);
        assert_eq!(panes.list.len(), 2);
        panes.active().pane_request = Some(Request::End("critic".into()));
        panes.settle(&host);
        assert_eq!((panes.list.len(), panes.active, panes.peek), (1, 0, None));
    }

    #[tokio::test]
    async fn a_click_where_a_hidden_pane_stood_never_moves_the_keys_behind_the_screen() {
        let host = Arc::new(crate::Standalone) as Arc<dyn Host>;
        let (mut panes, _inlets) = split();
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 3,
            number: 3,
            resume: false,
            tree: None,
            role: None,
            background: None,
            restore: None,
            reports: sender,
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        assert_eq!(panes.visible(), 3);
        let at = panes.list.iter().position(|pane| pane.number == 3).unwrap();
        panes.list[at].area = Rect::new(80, 0, 40, 20);
        panes.focus(at);
        panes.active().pane_request = Some(Request::Hide);
        panes.settle(&host);
        let hidden = panes.list.iter().position(|pane| pane.number == 3).unwrap();
        assert!(panes.list[hidden].background && panes.list[hidden].area == Rect::default());
        panes.on_terminal(TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 100,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
        assert!(!panes.list[panes.active].background);
        panes.focus(hidden);
        assert!(!panes.list[panes.active].background, "focus refuses it");
    }

    #[tokio::test]
    async fn a_window_keeps_its_layout_and_gets_it_back_pane_by_pane() {
        let dir = std::env::temp_dir().join(format!("glut-kept-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The main pane, two beside it, and an agent behind them.
        let (mut panes, _inlets) = split();
        panes.cwd = dir.clone();
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            looping: None,
            id: 3,
            number: 3,
            resume: false,
            tree: None,
            role: None,
            background: None,
            restore: None,
            reports: sender.clone(),
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        panes.building = Some(Building {
            looping: None,
            id: 4,
            number: 4,
            resume: false,
            tree: None,
            role: None,
            background: Some("critic".into()),
            restore: None,
            reports: sender.clone(),
            arena: None,
        });
        panes.on_built(Ok(session::for_tests()));
        panes.grid.share = 60;
        let home = panes.list[0].app.parts.home.clone();
        panes.keep_layout();
        let kept = crate::layout::load(&home, &dir).unwrap();
        assert_eq!(kept.summary(), "3 panes, 1 agent");
        assert_eq!(kept.share, 60);
        assert_eq!(kept.panes[3].agent.as_deref(), Some("critic"));
        // A new window of one pane takes it back: each kept pane built in turn, then the split as it stood.
        let (first, reports) = app();
        let (mut back, _inlets) = Panes::new(first, reports, true, Servers::Owner);
        back.list[0].app.parts = Arc::clone(&panes.list[0].app.parts);
        back.cwd = dir.clone();
        back.offer_layout();
        assert!(back.active().picker.is_some(), "offered once");
        back.active().picker = None;
        back.restore();
        let mut next_id = 10;
        while let Some(at) = back.restoring.as_mut().and_then(|r| r.left.pop_front()) {
            let kept = back.restoring.as_ref().unwrap().layout.panes[at].clone();
            back.restoring.as_mut().unwrap().building = Some(at);
            back.building = Some(Building {
                looping: None,
                id: next_id,
                number: u8::try_from(at + 1).unwrap(),
                resume: false,
                tree: None,
                role: None,
                background: kept.agent,
                restore: Some(kept.session),
                reports: sender.clone(),
                arena: None,
            });
            back.on_built(Ok(session::for_tests()));
            next_id += 1;
        }
        back.restore_step(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert!(back.restoring.is_none());
        assert_eq!(
            back.grid,
            Grid {
                main: 1,
                others: vec![10, 11],
                share: 60
            },
            "the grid as it stood"
        );
        assert_eq!((back.visible(), back.list.len()), (3, 4));
        // A window ending with one pane forgets it.
        let (single, reports) = app();
        let (mut single, _inlets) = Panes::new(single, reports, true, Servers::Owner);
        single.cwd = dir.clone();
        single.list[0].app.parts = Arc::clone(&panes.list[0].app.parts);
        single.keep_layout();
        assert!(
            crate::layout::load(&home, &dir).is_some(),
            "kept unless it was offered"
        );
        single.offered = true;
        single.keep_layout();
        assert!(crate::layout::load(&home, &dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn a_host_that_keeps_one_pane_is_not_split() {
        let (first, reports) = app();
        let (mut panes, _inlets) = Panes::new(first, reports, false, Servers::Owner);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(panes.building.is_none() && panes.list.len() == 1);
    }

    #[tokio::test]
    async fn each_pane_has_its_footer_with_its_number_and_the_cursor_is_the_active_ones() {
        let (mut panes, _inlets) = split();
        for pane in &mut panes.list {
            pane.app.separators = crate::footer::Separators::Plain;
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(161, 30)).unwrap();
        terminal
            .draw(|frame| crate::view::draw_panes(frame, &mut panes))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16, from: u16, to: u16| {
            (from..to)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        };
        assert!(row(29, 0, 80).starts_with(" 1 "), "{}", row(29, 0, 80));
        assert!(row(29, 81, 161).starts_with(" 2 "), "{}", row(29, 81, 161));
        assert_eq!(row(10, 80, 81), "│");
        let cursor = terminal.get_cursor_position().unwrap();
        assert!(cursor.x > 80, "the active pane's cursor: {cursor:?}");
    }

    #[test]
    fn the_grid_beside_the_main_pane_fills_two_rows_and_takes_one_out_evenly() {
        assert_eq!(
            (1..=6).map(Grid::rows).collect::<Vec<_>>(),
            [(1, 0), (1, 1), (2, 1), (2, 2), (3, 2), (3, 3)]
        );
        let mut grid = Grid::new(1);
        grid.others = vec![2, 3, 4, 5, 6];
        // 3 + 2 beside the main half: the top row's three need 3 × 60 and two columns, the main half as much.
        assert_eq!(grid.needs(60), 365);
        grid.remove(4);
        assert_eq!(grid.others, [2, 3, 5, 6]);
        grid.remove(1);
        assert_eq!(
            (grid.main, grid.order()),
            (2, vec![2, 3, 5, 6]),
            "the first of the others takes the main place"
        );
        assert!(
            grid.swap(5) && grid.order() == [5, 3, 2, 6],
            "trading places"
        );
        assert!(!grid.swap(9));
    }

    #[test]
    fn a_window_too_small_for_its_panes_shows_them_as_tabs() {
        let mut grid = Grid::new(1);
        grid.others = vec![2, 3, 4];
        // Wide: the right side in a grid, no tabs.
        let placed = grid.place(Rect::new(0, 0, 200, 40), 2, 1);
        assert!(placed.tabs.is_none() && placed.areas.len() == 4);
        // The right side too narrow for two columns of 40: its panes as tabs, the one last in front shown whole.
        let placed = grid.place(Rect::new(0, 0, 120, 40), 3, 1);
        let tabs = placed.tabs.unwrap();
        assert_eq!((tabs.panes.clone(), tabs.shown), (vec![2, 3, 4], 3));
        assert_eq!(
            placed.areas,
            [(1, Rect::new(0, 0, 60, 40)), (3, Rect::new(61, 1, 59, 39))]
        );
        // The pane with the keys is always the one shown, whatever was in front before.
        let placed = grid.place(Rect::new(0, 0, 120, 40), 3, 4);
        assert_eq!(placed.tabs.unwrap().shown, 4);
        // Too narrow for two panes at all: every one a tab, the active one whole.
        let placed = grid.place(Rect::new(0, 0, 70, 40), 3, 1);
        assert_eq!(placed.tabs.unwrap().panes, [1, 2, 3, 4]);
        assert_eq!(placed.areas, [(1, Rect::new(0, 1, 70, 39))]);
        assert!(placed.border.is_none());
        // A tab is found where it was drawn.
        let names = [(1, "1".to_owned()), (2, "2 reviewer".to_owned())];
        let spots = tab_spots(Rect::new(0, 0, 70, 1), &names, 1);
        assert_eq!(
            spots,
            [(1, Rect::new(0, 0, 3, 1)), (2, Rect::new(4, 0, 12, 1))]
        );
        assert_eq!(tab_under(Rect::new(0, 0, 70, 1), &names, 1, 6), Some(2));
        assert_eq!(tab_under(Rect::new(0, 0, 70, 1), &names, 1, 3), None);
        // A line too short for all: it starts late enough to hold the tab shown.
        let spots = tab_spots(Rect::new(0, 0, 14, 1), &names, 2);
        assert_eq!(spots, [(2, Rect::new(0, 0, 12, 1))]);
    }

    #[test]
    fn numbers_read_as_a_list() {
        assert_eq!(numbers(&[2]), "2");
        assert_eq!(numbers(&[2, 3]), "2 and 3");
        assert_eq!(numbers(&[1, 2, 3]), "1, 2 and 3");
    }
}
