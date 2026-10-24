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

/// The most panes (T36d): four, each split again as the owner likes.
pub const MOST: usize = 4;
/// How many messages models send each other without the owner's word before the next waits for it (T36c).
pub const MOST_CHAIN: u32 = 6;
/// How many messages one session's model may send to other panes (T36c).
pub const MOST_TELLS: u32 = 30;
/// The most background agents (T36e), beside the panes.
pub const MOST_BACKGROUND: usize = 4;
/// How long a first `/quit` or `/close` stays armed while a pane works: the second within it goes on.
const ARMED: Duration = Duration::from_secs(10);

/// Where a new pane goes beside the active one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Beside it, before it.
    Left,
    /// Beside it, after it — `/split` without a side.
    Right,
    /// Above it.
    Up,
    /// Below it.
    Down,
}

impl Side {
    /// `l`, `r`, `u`, `d` or the words; `None` for anything else.
    pub fn parse(word: &str) -> Option<Side> {
        match word {
            "l" | "left" => Some(Side::Left),
            "r" | "right" => Some(Side::Right),
            "u" | "up" => Some(Side::Up),
            "d" | "down" => Some(Side::Down),
            _ => None,
        }
    }

    fn stacked(self) -> bool {
        matches!(self, Side::Up | Side::Down)
    }

    fn before(self) -> bool {
        matches!(self, Side::Left | Side::Up)
    }
}

/// How the panes stand (T36d): a pane, or a split of panes side by side or one above the other — a pane split again
/// in the other direction becomes a split of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    /// A pane, by its session's id.
    Pane(u64),
    Split {
        /// One above the other rather than side by side.
        stacked: bool,
        /// Two or more, in reading order.
        parts: Vec<Node>,
    },
}

impl Node {
    /// Puts pane `new` at `side` of pane `at`: beside it in a split that already runs that way, else in a split of
    /// their own where `at` stood. `false` when `at` is nowhere.
    fn split(&mut self, at: u64, new: u64, side: Side) -> bool {
        match self {
            Node::Pane(id) if *id == at => {
                let (first, second) = if side.before() { (new, at) } else { (at, new) };
                *self = Node::Split {
                    stacked: side.stacked(),
                    parts: vec![Node::Pane(first), Node::Pane(second)],
                };
                true
            }
            Node::Pane(_) => false,
            Node::Split { stacked, parts } => {
                if *stacked == side.stacked()
                    && let Some(place) = parts.iter().position(|part| *part == Node::Pane(at))
                {
                    let place = if side.before() { place } else { place + 1 };
                    parts.insert(place, Node::Pane(new));
                    return true;
                }
                parts.iter_mut().any(|part| part.split(at, new, side))
            }
        }
    }

    /// Takes pane `id` out; a split left with one part becomes that part, and a part that becomes a split running
    /// the way its parent's does gives its parts to the parent — the room is shared evenly again.
    fn remove(&mut self, id: u64) {
        if let Node::Split { stacked, parts } = self {
            parts.retain(|part| *part != Node::Pane(id));
            for part in parts.iter_mut() {
                part.remove(id);
            }
            let direction = *stacked;
            *parts = std::mem::take(parts)
                .into_iter()
                .flat_map(|part| match part {
                    Node::Split { stacked, parts } if stacked == direction => parts,
                    other => vec![other],
                })
                .collect();
            if parts.len() == 1 {
                *self = parts.remove(0);
            }
        }
    }

    /// The panes in reading order: left to right, top to bottom.
    fn order(&self, out: &mut Vec<u64>) {
        match self {
            Node::Pane(id) => out.push(*id),
            Node::Split { parts, .. } => parts.iter().for_each(|part| part.order(out)),
        }
    }

    /// The narrowest this keeps every pane at least `least` columns wide: side by side the room is shared evenly,
    /// so it takes as many times the widest part's need, with a column between each two.
    fn needs(&self, least: u16) -> u16 {
        match self {
            Node::Pane(_) => least,
            Node::Split {
                stacked: true,
                parts,
            } => parts
                .iter()
                .map(|part| part.needs(least))
                .max()
                .unwrap_or(least),
            Node::Split {
                stacked: false,
                parts,
            } => {
                let count = parts.len() as u16;
                let widest = parts
                    .iter()
                    .map(|part| part.needs(least))
                    .max()
                    .unwrap_or(least);
                widest
                    .saturating_mul(count)
                    .saturating_add(count.saturating_sub(1))
            }
        }
    }

    /// Each pane's area in `area`, and the columns parting panes side by side.
    fn place(&self, area: Rect, areas: &mut Vec<(u64, Rect)>, lines: &mut Vec<Rect>) {
        match self {
            Node::Pane(id) => areas.push((*id, area)),
            Node::Split { stacked, parts } => {
                let count = parts.len() as u16;
                for (at, part) in parts.iter().enumerate() {
                    let at = at as u16;
                    let last = at + 1 == count;
                    let part_area = if *stacked {
                        let height = area.height / count;
                        Rect {
                            y: area.y + at * height,
                            height: if last {
                                area.height - at * height
                            } else {
                                height
                            },
                            ..area
                        }
                    } else {
                        // One column between each two panes for the line that parts them.
                        let width = area.width.saturating_sub(count - 1) / count;
                        let x = area.x + at * (width + 1);
                        if at > 0 {
                            lines.push(Rect::new(x - 1, area.y, 1, area.height));
                        }
                        Rect {
                            x,
                            width: if last {
                                area.width.saturating_sub(at * (width + 1))
                            } else {
                                width
                            },
                            ..area
                        }
                    };
                    part.place(part_area, areas, lines);
                }
            }
        }
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

/// What a pane asks of the screen: its keys (`⌃]`, `⌃X 2`, `⌃X 3`, `⌃X 0`), commands (`/split`, `/close`) and picks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A new pane at `side` working at `place`, playing `role` (T36c); `resume`: an earlier session in it, picked from
    /// the list.
    Split {
        side: Side,
        resume: bool,
        place: Place,
        role: Option<String>,
    },
    /// Close the asking pane.
    Close,
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
    /// The asking pane's last plan with its task list, to be built in pane `to`.
    SendPlan { to: Option<String> },
    /// A background agent (T36e): a session with no room on the screen, named `name` or after its role.
    Background {
        name: Option<String>,
        role: Option<String>,
        place: Place,
    },
    /// A background agent brought forward as a pane.
    Show(String),
    /// The asking pane sent to the background.
    Hide,
    /// A background agent's stream shown over the asking pane, only to read; `None` closes it.
    Peek(Option<String>),
    /// A background agent ended where it is, its turn stopped (`/close <name>`).
    End(String),
}

/// `/send`'s words: maybe a pane first (a number, a name, a role — the screen knows which word names one), then the
/// text — or `plan`.
///
/// # Errors
///
/// What `/send` takes, when nothing is to be sent.
pub fn parse_send(rest: &str) -> Result<Request, String> {
    let rest = rest.trim();
    let empty =
        || Err("/send <text> — or /send plan; a pane's number or name first picks it".to_owned());
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
        None if rest.is_empty() || rest.parse::<u8>().is_ok() => empty(),
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
                None => return Err("/bg as <role>: a role's name (/role lists them)".to_owned()),
            },
            "tree" => {
                let branch = words.next_if(|next| !matches!(*next, "here" | "tree" | "as"));
                place = Place::Tree(branch.unwrap_or_default().to_owned());
            }
            given if name.is_none() && is_name(given) => name = Some(given.to_owned()),
            other => {
                return Err(format!(
                    "/bg {other}: a name (letters, digits, dashes), as <role>, here or tree [branch]"
                ));
            }
        }
    }
    Ok(Request::Background { name, role, place })
}

/// Whether `word` can name a background agent: lower-case letters, digits and dashes, a letter first — never a pane's
/// number.
fn is_name(word: &str) -> bool {
    // `/send` and `/bg` read these words as their own.
    const RESERVED: [&str; 4] = ["plan", "here", "tree", "as"];
    !RESERVED.contains(&word)
        && word.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && word.len() <= 32
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `/split`'s words: a side (`l r u d`), `resume`, and where — `here`, or `tree` with an optional branch name.
///
/// # Errors
///
/// What `/split` takes, for a word it does not.
pub fn parse_split(rest: &str) -> Result<Request, String> {
    let mut side = Side::Right;
    let mut resume = false;
    let mut place = Place::Ask;
    let mut role = None;
    let mut words = rest.split_whitespace().peekable();
    while let Some(word) = words.next() {
        match (word, Side::parse(word)) {
            (_, Some(given)) => side = given,
            ("resume", None) => resume = true,
            ("here", None) => place = Place::Here,
            ("as", None) => match words.next() {
                Some(name) => role = Some(name.to_owned()),
                None => return Err("/split as <role>: a role's name (/role lists them)".to_owned()),
            },
            ("tree", None) => {
                let branch = words.next_if(|next| {
                    Side::parse(next).is_none()
                        && !matches!(*next, "resume" | "here" | "tree" | "as")
                });
                place = Place::Tree(branch.unwrap_or_default().to_owned());
            }
            (other, None) => {
                return Err(format!(
                    "/split {other}: l, r, u or d; resume; here or tree [branch]; as <role>"
                ));
            }
        }
    }
    Ok(Request::Split {
        side,
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
}

/// A pane whose session is still being built.
struct Building {
    id: u64,
    number: u8,
    side: Side,
    resume: bool,
    tree: Option<crate::worktree::Tree>,
    role: Option<String>,
    /// The name of a background agent (T36e); `None` for a pane.
    background: Option<String>,
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
    layout: Node,
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
        let panes = Panes {
            list: vec![Pane {
                id: 1,
                number: 1,
                name: None,
                background: false,
                app,
                area: Rect::default(),
                tree: None,
            }],
            active: 0,
            layout: Node::Pane(1),
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

    /// How a pane is named to the owner and the models: "pane 2", or a background agent's name.
    fn label(&self, at: usize) -> String {
        let pane = &self.list[at];
        match (&pane.name, pane.background) {
            (Some(name), true) => name.clone(),
            _ => format!("pane {}", pane.number),
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
        // The Agents tab of every pane lists the agents in the background (T36e).
        let background: Vec<String> = self
            .list
            .iter()
            .filter(|pane| pane.background)
            .map(|pane| {
                let app = &pane.app;
                let state = if app.waits_for_owner() {
                    "asks"
                } else if !app.held.is_empty() {
                    "holds a message"
                } else if app.working() {
                    "works"
                } else {
                    "rests"
                };
                let cost = app
                    .cost
                    .map(|cost| format!(" · ${cost:.4}"))
                    .unwrap_or_default();
                let cache = match (app.cache_seen, app.warm_until) {
                    (true, Some(until)) if until > Instant::now() => " · cache ●",
                    (true, _) => " · cache ○",
                    _ => "",
                };
                format!(
                    "{} · {} · {state}{cost}{cache}",
                    pane.name.as_deref().unwrap_or("agent"),
                    app.role.as_deref().unwrap_or("all-rounder")
                )
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
        if let Report::Tell { to, text, reply } = report {
            let _ = reply.send(self.tell(id, to, text));
            return;
        }
        if let Some(pane) = self.list.iter_mut().find(|pane| pane.id == id) {
            pane.app.on_report(report);
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
        let delivered = self.list[target]
            .app
            .deliver_model(message, from, may_start, sender)?;
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

    /// A key, a paste or the mouse: the active pane's, but a click moves into the pane under it (the click itself
    /// goes nowhere) and the wheel goes to the pane under it.
    pub fn on_terminal(&mut self, event: TermEvent) {
        // A background agent's stream over the pane is only to read: Esc (or ⌃C) closes it, nothing else goes through.
        if self.peek.is_some() {
            if let TermEvent::Key(key) = &event
                && (key.code == crossterm::event::KeyCode::Esc
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
                    self.active().say("one pane — /split splits the window");
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
                Request::CloseAs(leaving) => self.leave(leaving),
                Request::Split {
                    side,
                    resume,
                    place,
                    role,
                } => self.start(side, resume, place, role, host),
                Request::Send { to, text, whole } => {
                    // A first word naming no pane or agent is the message's own (`/send 3 files changed`).
                    let named = to
                        .as_deref()
                        .is_some_and(|word| self.resolve(word, self.active).is_some());
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
                Request::Background { name, role, place } => {
                    self.start_background(name, role, place, host);
                }
                Request::Show(name) => self.show(&name),
                Request::Hide => self.hide(),
                Request::Peek(None) => self.close_peek(),
                Request::End(name) => self.end_background(&name),
                Request::Peek(Some(name)) => match self.resolve_background(&name) {
                    Some(at) => self.peek = Some(self.list[at].id),
                    _ => self.active().tell(format!(
                        "no background agent {} — /peek <name>",
                        crate::transcript::clean(&name)
                    )),
                },
            }
        }
    }

    /// Whether the screen ends: a closed window or a kill ends every pane; a `/quit` of any pane ends them all —
    /// said once more first while another works.
    pub fn quitting(&mut self) -> bool {
        if self.list.iter().any(|pane| pane.app.abandoned) {
            self.list.iter_mut().for_each(|pane| pane.app.abandon());
            return true;
        }
        let Some(at) = self.list.iter().position(|pane| pane.app.quit) else {
            return false;
        };
        let working: Vec<String> = (0..self.list.len())
            .filter(|other| *other != at && self.list[*other].app.working())
            .map(|other| self.label(other))
            .collect();
        let armed = self.quit_armed.is_some_and(|since| since.elapsed() < ARMED);
        if working.is_empty() || armed {
            return true;
        }
        self.quit_armed = Some(Instant::now());
        let app = &mut self.list[at].app;
        app.quit = false;
        app.tell(format!(
            "{} still works — /quit again ends it too",
            listed(&working)
        ));
        false
    }

    /// Closes the active pane; a turn under way is stopped — said once first. The last pane is not closed.
    fn close(&mut self) {
        if !self.split() {
            return self.active().tell("the last pane — /quit ends Glut");
        }
        let number = self.list[self.active].number;
        let armed = self
            .close_armed
            .is_some_and(|(armed, since)| armed == number && since.elapsed() < ARMED);
        if self.active().working() && !armed {
            self.close_armed = Some((number, Instant::now()));
            return self
                .active()
                .tell("this pane works — /close again stops it and closes the pane");
        }
        self.close_armed = None;
        if let Some(tree) = self.list[self.active].tree.clone() {
            // Git goes over the worktree only once nothing writes there any more.
            if self.active().working() {
                self.active().stop_turn();
                return self
                    .active()
                    .tell("its turn is stopping — /close again once it has stopped, to say how the worktree ends");
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
        let Some(tree) = self.list[self.active].tree.clone() else {
            return self.remove_active();
        };
        if self.active().working() {
            return self
                .active()
                .tell("this pane works again — /close once it is done");
        }
        let standing = crate::worktree::standing(&tree);
        // What cannot be read is taken as work a discard would lose.
        let loses_work = standing
            .as_ref()
            .map_or(true, |standing| standing.loses_work());
        let done = match leaving {
            Leaving::Keep => Ok(format!(
                "its worktree {} stays on {} — glut there goes on with /resume",
                tree.place.display(),
                tree.branch
            )),
            Leaving::TakeOver => crate::worktree::take_over(&tree),
            Leaving::Discard if loses_work => {
                let picker = discard_picker(&tree, &standing);
                self.active().picker = Some(picker);
                return;
            }
            Leaving::Discard | Leaving::DiscardSure => crate::worktree::remove(&tree, true)
                .map(|()| format!("{} and its worktree are gone", tree.branch)),
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
        self.layout.remove(pane.id);
        let number = pane.number;
        let stopped = pane.app.working();
        pane.app.stop_turn();
        // The keys go to a pane on the screen, never to an agent in the background.
        let near = self.active.min(self.list.len() - 1);
        let next = (0..self.list.len())
            .filter(|at| !self.list[*at].background)
            .min_by_key(|at| at.abs_diff(near))
            .map_or(self.list[near].id, |at| self.list[at].id);
        self.reorder();
        self.focus(self.at(next));
        // Alone again: the side panel comes back as the owner keeps it.
        if !self.split() {
            let home = self.active().parts.home.clone();
            self.active().panel = crate::prefs::Prefs::load(&home).panel;
        }
        // A worktree's session is found from its own folder; the leaving said where.
        let kept = match (stopped, &pane.tree) {
            (true, _) => "its turn was stopped; the session is kept as it was before that turn",
            (false, Some(_)) => "its session is kept",
            (false, None) => "/resume finds its session",
        };
        self.active().tell(format!("pane {number} closed — {kept}"));
    }

    /// Starts a new pane beside the active one: a session in the same folder, on the active pane's provider and
    /// model, in build — built beside the screen.
    fn start(
        &mut self,
        side: Side,
        resume: bool,
        place: Place,
        role: Option<String>,
        host: &Arc<dyn Host>,
    ) {
        self.start_as(side, resume, place, role, None, host);
    }

    /// [`Self::start`], or — with `background` a name — an agent in the background (T36e).
    fn start_as(
        &mut self,
        side: Side,
        resume: bool,
        place: Place,
        role: Option<String>,
        background: Option<String>,
        host: &Arc<dyn Host>,
    ) {
        if !self.may_split {
            return self
                .active()
                .tell("this session's host keeps one pane: its studio splits its own windows");
        }
        if background.is_none() && self.visible() >= MOST {
            return self.active().tell(format!("at most {MOST} panes for now"));
        }
        if self.building.is_some() {
            return self.active().tell("a pane is starting already");
        }
        // A role named wrong ends the split here, rather than open a pane with every tool.
        if let Some(role) = &role
            && self.active().parts.config.agents.get(role).is_none()
        {
            let known = self.active().parts.config.agents.known();
            return self.active().transcript.note(
                format!("no role {role}: {known} (/role lists them)"),
                Tone::Fault,
            );
        }
        let number = (1..=u8::MAX)
            .find(|number| self.list.iter().all(|pane| pane.number != *number))
            .unwrap_or(u8::MAX);
        let cwd = self.active().parts.config.cwd.clone();
        let repo = match &place {
            Place::Here => Ok(None),
            _ => crate::worktree::repo_of(&cwd),
        };
        let tree = match (place, repo) {
            (Place::Here, _) | (Place::Ask, Ok(None)) => None,
            (Place::Ask, Err(reason)) => {
                self.active().tell(format!("no worktree offered: {reason}"));
                None
            }
            // A background agent works here unless told `tree`: it has no pane to ask in.
            (Place::Ask, Ok(Some(_))) if background.is_some() => None,
            (Place::Ask, Ok(Some(repo))) => {
                let picker = where_picker(side, resume, role.as_deref(), &repo, number);
                self.active().picker = Some(picker);
                return;
            }
            (Place::Tree(_), Err(reason)) => {
                return self.active().transcript.note(reason, Tone::Fault);
            }
            (Place::Tree(_), Ok(None)) => {
                return self
                    .active()
                    .tell("this folder is in no git repository: /split here");
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
        app.tell(format!("pane {number} starts…"));
        let (reports, inlet) = mpsc::unbounded_channel();
        forward(id, inlet, self.reports.clone());
        self.building = Some(Building {
            id,
            number,
            side,
            resume,
            tree,
            role,
            background,
            reports: reports.clone(),
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
                // A worktree made for it holds nothing yet: it goes with the pane that never came.
                let gone = match &building.tree {
                    Some(tree) => match crate::worktree::remove(tree, true) {
                        Ok(()) => format!("; its worktree {} is gone again", tree.branch),
                        Err(left) => {
                            format!("; its worktree stays at {} ({left})", tree.place.display())
                        }
                    },
                    None => String::new(),
                };
                return self.active().transcript.note(
                    format!("pane {} could not start: {reason}{gone}", building.number),
                    Tone::Fault,
                );
            }
        };
        let mut app = App::new(Arc::new(parts), engine, building.reports);
        if building.resume {
            app.picker = Some(app.session_picker());
        }
        if let Err(reason) = app.set_role(building.role.as_deref()) {
            app.transcript.note(reason, Tone::Fault);
        }
        // A background agent stands in no split: it waits for its first message, reached by its name.
        if let Some(name) = building.background {
            app.panel = Some(false);
            self.list.push(Pane {
                id: building.id,
                number: building.number,
                name: Some(name.clone()),
                background: true,
                app,
                area: Rect::default(),
                tree: building.tree,
            });
            self.reorder();
            return self.active().tell(format!(
                "{name} waits in the background — /send {name} …, /peek {name}, /show {name}"
            ));
        }
        // Panes leave little room beside them: the side panel starts hidden (⌃B brings it, where there is room).
        app.panel = Some(false);
        let current = &mut self.list[self.active].app;
        current.panel = Some(false);
        let busy_here =
            !current.input.is_empty() || current.waits_for_owner() || current.picker.is_some();
        let active_id = self.list[self.active].id;
        let placed = self.layout.split(active_id, building.id, building.side);
        debug_assert!(placed, "the active pane is always in the layout");
        self.list.push(Pane {
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
            self.active()
                .tell(format!("pane {number} is ready — ⌃P goes there"));
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
        let Some(target) = target else {
            let said = match (&to, self.list.len()) {
                (_, 1) => "one pane — /split or /bg first".to_owned(),
                (Some(word), _) => format!("no pane or agent {}", crate::transcript::clean(word)),
                (None, _) => "which one? /send <number or name> …".to_owned(),
            };
            return self.active().tell(said);
        };
        let number = self.list[target].number;
        let label = self.label(target);
        let plan = text.is_none();
        let text = match text {
            Some(text) => text,
            None => match self.active().plan_to_build() {
                Some(plan) => plan,
                None => return self.active().tell("no plan yet: plan mode (⇧⇥) makes one"),
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
            Delivered::Now => "it starts on it now",
            Delivered::AfterTurn => "it takes it after its turn",
            Delivered::WhenFree => "it takes it once it is free",
            Delivered::OnEnter => {
                "it waits there for Enter (a list was open or messages were waiting)"
            }
        };
        self.active().tell(format!("sent to {label} — {when}"));
    }

    /// `/bg`: a session with no room on the screen (T36e), named `name` or after its role (`reviewer`, a second
    /// `reviewer-2`), built as a pane is — on the active pane's provider and model, in build.
    fn start_background(
        &mut self,
        name: Option<String>,
        role: Option<String>,
        place: Place,
        host: &Arc<dyn Host>,
    ) {
        let background = self.list.iter().filter(|pane| pane.background).count();
        if background >= MOST_BACKGROUND {
            return self.active().tell(format!(
                "at most {MOST_BACKGROUND} agents in the background"
            ));
        }
        let base = name
            .or_else(|| role.clone())
            .filter(|base| is_name(base))
            .unwrap_or_else(|| "agent".to_owned());
        let taken = |candidate: &str| {
            self.list.iter().any(|pane| {
                pane.name.as_deref() == Some(candidate)
                    || pane.app.role.as_deref() == Some(candidate)
            })
        };
        let name = if taken(&base) {
            (2..)
                .map(|n| format!("{base}-{n}"))
                .find(|candidate| !taken(candidate))
                .unwrap_or(base)
        } else {
            base
        };
        self.start_as(Side::Right, false, place, role, Some(name), host);
    }

    /// `/show <name>`: a background agent brought forward as a pane, to the right of the active one.
    fn show(&mut self, name: &str) {
        let Some(at) = self.resolve_background(name) else {
            return self.active().tell(format!(
                "no background agent {} — ⌃X A lists them",
                crate::transcript::clean(name)
            ));
        };
        if self.visible() >= MOST {
            return self
                .active()
                .tell(format!("at most {MOST} panes: /hide or /close one first"));
        }
        let id = self.list[at].id;
        let beside = self.list[self.active].id;
        // Panes leave little room beside them: the side panel goes, as when a pane is split off.
        self.active().panel = Some(false);
        self.layout.split(beside, id, Side::Right);
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
            return self.active().tell(format!(
                "no background agent {} — ⌃X A lists them",
                crate::transcript::clean(name)
            ));
        };
        let label = self.label(at);
        if self.list[at].tree.is_some() {
            return self.active().tell(format!(
                "{label} works in a worktree: /show {label}, then /close there asks how it ends"
            ));
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
            "its turn was stopped; the session is kept as it was before that turn"
        } else {
            "/resume finds its session"
        };
        self.active().tell(format!("{label} ended — {kept}"));
    }

    /// `/hide`: the active pane goes to the background with its memory; it keeps a name to be reached by.
    fn hide(&mut self) {
        if self.visible() <= 1 {
            return self
                .active()
                .tell("the last pane stays: /bg starts an agent in the background");
        }
        if self.list.iter().filter(|pane| pane.background).count() >= MOST_BACKGROUND {
            return self.active().tell(format!(
                "at most {MOST_BACKGROUND} agents in the background"
            ));
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
        self.layout.remove(id);
        self.list[at].background = true;
        // Where it stood is another pane's now: the mouse must not find it there.
        self.list[at].area = Rect::default();
        let next = (0..self.list.len())
            .find(|other| !self.list[*other].background)
            .map(|other| self.list[other].id)
            .unwrap_or(id);
        self.reorder();
        let label = self.label(self.at(id));
        self.focus(self.at(next));
        // Alone again: the side panel comes back as the owner keeps it.
        if !self.split() {
            let home = self.active().parts.home.clone();
            self.active().panel = crate::prefs::Prefs::load(&home).panel;
        }
        self.active().tell(format!(
            "{label} works on in the background — /send {label} …, /peek {label}, /show {label}"
        ));
    }

    /// Moves the keys to pane `at`; what it asks counts as shown from now.
    fn focus(&mut self, at: usize) {
        // The keys never go to a pane drawn nowhere.
        if self.list[at].background {
            return;
        }
        self.active = at;
        self.active().shown_again();
    }

    /// Closes a peek; a question that came up under it counts as shown only from now.
    fn close_peek(&mut self) {
        self.peek = None;
        self.active().shown_again();
    }

    /// The list in the layout's reading order, so `⌃P` goes round as the panes stand.
    fn reorder(&mut self) {
        let mut order = Vec::new();
        self.layout.order(&mut order);
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
        self.layout.needs(least)
    }

    /// Each pane's area in `area`, in the list's order, and the columns that part panes side by side.
    pub fn areas(&self, area: Rect) -> (Vec<Rect>, Vec<Rect>) {
        if !self.split() {
            return (vec![area], Vec::new());
        }
        let mut placed = Vec::new();
        let mut lines = Vec::new();
        self.layout.place(area, &mut placed, &mut lines);
        let areas = self
            .list
            .iter()
            .map(|pane| {
                placed
                    .iter()
                    .find(|(id, _)| *id == pane.id)
                    .map_or(Rect::default(), |(_, area)| *area)
            })
            .collect();
        (areas, lines)
    }
}

/// Where a new pane in a git repository works: this folder, or a worktree whose branch the owner may rename first
/// (the command goes into the input).
fn where_picker(
    side: Side,
    resume: bool,
    role: Option<&str>,
    repo: &crate::worktree::Repo,
    number: u8,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let branch = crate::worktree::proposed_branch(repo, number);
    let letter = match side {
        Side::Left => "l",
        Side::Right => "r",
        Side::Up => "u",
        Side::Down => "d",
    };
    let resume_word = if resume { " resume" } else { "" };
    let role_words = role.map_or(String::new(), |role| format!(" as {role}"));
    let items = vec![
        Item::plain(
            "this folder",
            "both panes change the same files",
            Pick::Pane(Request::Split {
                side,
                resume,
                place: Place::Here,
                role: role.map(str::to_owned),
            }),
        ),
        Item::plain(
            "a new worktree",
            format!("on the branch {branch} — ⏎ puts it in the input to rename first"),
            Pick::Input(format!(
                "/split {letter}{resume_word}{role_words} tree {branch}"
            )),
        ),
    ];
    Picker::new(format!("Pane {number}: where does it work?"), items, None).guarded()
}

/// How a worktree's pane ends, with what it holds said.
fn leaving_picker(
    tree: &crate::worktree::Tree,
    standing: Result<crate::worktree::Standing, String>,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let held = match &standing {
        Ok(standing) => standing.said(),
        Err(_) => "what it holds cannot be read".to_owned(),
    };
    let into = tree
        .into
        .as_deref()
        .unwrap_or("no branch: merge it by hand");
    let items = vec![
        Item::plain(
            "keep",
            format!("the worktree and {} stay ({held})", tree.branch),
            Pick::Pane(Request::CloseAs(Leaving::Keep)),
        ),
        Item::plain(
            "take over",
            format!("merge {} ({held}) into {into}", tree.branch),
            Pick::Pane(Request::CloseAs(Leaving::TakeOver)),
        ),
        Item::plain(
            "discard",
            format!("remove the worktree and {} ({held})", tree.branch),
            Pick::Pane(Request::CloseAs(Leaving::Discard)),
        ),
    ];
    Picker::new("Closing this worktree's pane", items, None).guarded()
}

/// The second question before work goes.
fn discard_picker(
    tree: &crate::worktree::Tree,
    standing: &Result<crate::worktree::Standing, String>,
) -> crate::picker::Picker {
    use crate::picker::{Item, Pick, Picker};
    let items = vec![
        Item::plain("no", "keep it — the pane stays open", Pick::Shown),
        Item::plain(
            "yes, discard",
            match standing {
                Ok(standing) => format!("{} is lost: {}", tree.branch, standing.said()),
                Err(reason) => format!("{} goes, whatever it holds ({reason})", tree.branch),
            },
            Pick::Pane(Request::CloseAs(Leaving::DiscardSure)),
        ),
    ];
    Picker::new(format!("Discard {}?", tree.branch), items, None).guarded()
}

/// The new pane's session: the owner's, in the active pane's folder, on its provider and model, with the first
/// session's servers, starting in build (the owner's flow: the first pane plans, the second builds).
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
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
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

    /// Two panes, the second built to the `side` of the first, without starting a real session.
    fn split(side: Side) -> (Panes, Inlets) {
        let (first, reports) = app();
        let (mut panes, inlets) = Panes::new(first, reports, true, Servers::Owner);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            id: 2,
            number: 2,
            side,
            resume: false,
            tree: None,
            role: None,
            background: None,
            reports: sender,
        });
        panes.on_built(Ok(session::for_tests()));
        (panes, inlets)
    }

    /// One pane and an agent in the background named `name`, built without starting a real session.
    fn with_background(name: &str) -> (Panes, Inlets) {
        let (first, reports) = app();
        let (mut panes, inlets) = Panes::new(first, reports, true, Servers::Owner);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            id: 2,
            number: 2,
            side: Side::Right,
            resume: false,
            tree: None,
            role: None,
            background: Some(name.to_owned()),
            reports: sender,
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
    async fn a_new_pane_takes_the_next_number_where_it_was_asked_for_and_the_keys() {
        let (panes, _inlets) = split(Side::Right);
        assert_eq!(
            (numbers_of(&panes), panes.active, panes.needs(60)),
            (vec![1, 2], 1, 121)
        );
        let (panes, _inlets) = split(Side::Left);
        assert_eq!(
            (numbers_of(&panes), panes.active),
            (vec![2, 1], 0),
            "2 stands left"
        );
        let (panes, _inlets) = split(Side::Down);
        assert_eq!(panes.needs(60), 60, "one above the other share the width");
        let (areas, lines) = panes.areas(Rect::new(0, 0, 100, 41));
        assert_eq!((areas[0].height, areas[1].y, areas[1].height), (20, 20, 21));
        assert!(lines.is_empty(), "one above the other: no column between");
        let (panes, _inlets) = split(Side::Right);
        let (areas, lines) = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(
            (areas[0].width, areas[1].x, areas[1].width),
            (50, 51, 50),
            "a column parts them"
        );
        assert_eq!(lines, [Rect::new(50, 0, 1, 40)]);
    }

    /// Pane `number` built at `side` of the active one, as `on_built` makes it.
    fn add(panes: &mut Panes, number: u8, side: Side) {
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            id: u64::from(number),
            number,
            side,
            resume: false,
            tree: None,
            role: None,
            background: None,
            reports: sender,
        });
        panes.on_built(Ok(session::for_tests()));
    }

    #[tokio::test]
    async fn a_pane_split_again_stands_where_asked_and_closing_one_closes_the_gap() {
        let (mut panes, _inlets) = split(Side::Right);
        // Pane 2 on the right, split below: 1 | (2 over 3).
        add(&mut panes, 3, Side::Down);
        assert_eq!(numbers_of(&panes), vec![1, 2, 3]);
        assert_eq!(panes.needs(60), 121);
        let (areas, lines) = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(areas[0], Rect::new(0, 0, 50, 40));
        assert_eq!(
            (areas[1], areas[2]),
            (Rect::new(51, 0, 50, 20), Rect::new(51, 20, 50, 20))
        );
        assert_eq!(lines.len(), 1);
        // Back to pane 1, split below too: (1 over 4) | (2 over 3); ⌃P goes round in reading order.
        panes.focus(0);
        add(&mut panes, 4, Side::Down);
        assert_eq!(numbers_of(&panes), vec![1, 4, 2, 3]);
        let (areas, _) = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(areas[1], Rect::new(0, 20, 50, 20));
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(panes.building.is_none(), "at most four");
        // Closing pane 2: pane 3 takes its whole column.
        panes.focus(2);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(numbers_of(&panes), vec![1, 4, 3]);
        let (areas, _) = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(areas[2], Rect::new(51, 0, 50, 40));
        assert_eq!(
            panes.list[panes.active].number, 3,
            "the keys go to a neighbour"
        );
    }

    #[tokio::test]
    async fn ctrl_bracket_moves_the_keys_and_a_click_moves_into_the_pane_under_it() {
        let (mut panes, _inlets) = split(Side::Right);
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
        let (mut panes, _inlets) = split(Side::Right);
        panes.list[0].app.working_for_tests();
        panes.list[1].app.quit = true;
        assert!(!panes.quitting(), "pane 1 still works");
        assert!(!panes.list[1].app.quit);
        panes.list[1].app.quit = true;
        assert!(panes.quitting(), "the second /quit ends all");
        let (mut panes, _inlets) = split(Side::Right);
        panes.active = 0;
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(numbers_of(&panes), vec![2], "pane 2 keeps its number");
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        assert_eq!(numbers_of(&panes), vec![2], "the last pane stays");
    }

    #[tokio::test]
    async fn a_kill_ends_every_pane_even_while_one_works() {
        let (mut panes, _inlets) = split(Side::Right);
        panes.list[0].app.working_for_tests();
        panes.list[1].app.abandon();
        assert!(panes.quitting());
        assert!(panes.list.iter().all(|pane| pane.app.abandoned));
    }

    #[tokio::test]
    async fn a_closed_sessions_late_report_reaches_no_pane_though_its_number_is_taken_again() {
        let (mut panes, _inlets) = split(Side::Right);
        let old = panes.list[1].id;
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('0'), KeyModifiers::NONE);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            id: 7,
            number: 2,
            side: Side::Right,
            resume: false,
            tree: None,
            role: None,
            background: None,
            reports: sender,
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
            id: 2,
            number: 2,
            side: Side::Left,
            resume: false,
            tree: None,
            role: None,
            background: None,
            reports: sender,
        });
        panes.on_built(Ok(session::for_tests()));
        assert_eq!(
            (numbers_of(&panes), panes.active),
            (vec![2, 1], 1),
            "pane 1 keeps the keys"
        );
    }

    #[test]
    fn split_reads_its_side_resume_and_where() {
        assert_eq!(
            parse_split("").unwrap(),
            Request::Split {
                side: Side::Right,
                resume: false,
                place: Place::Ask,
                role: None,
            }
        );
        assert_eq!(
            parse_split("d resume here").unwrap(),
            Request::Split {
                side: Side::Down,
                resume: true,
                place: Place::Here,
                role: None,
            }
        );
        assert_eq!(
            parse_split("tree glut/plan r").unwrap(),
            Request::Split {
                side: Side::Right,
                resume: false,
                place: Place::Tree("glut/plan".into()),
                role: None,
            }
        );
        assert_eq!(
            parse_split("l tree").unwrap(),
            Request::Split {
                side: Side::Left,
                resume: false,
                place: Place::Tree(String::new()),
                role: None,
            }
        );
        assert_eq!(
            parse_split("d as reviewer tree").unwrap(),
            Request::Split {
                side: Side::Down,
                resume: false,
                place: Place::Tree(String::new()),
                role: Some("reviewer".into()),
            }
        );
        assert!(parse_split("as").is_err());
        assert!(parse_split("sideways").is_err());
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
        assert_eq!(panes.active().input.text(), "/split d tree glut/pane-2");
        let _ = std::fs::remove_dir_all(repo.parent().unwrap());
    }

    #[tokio::test]
    async fn a_worktrees_pane_ends_as_picked_and_discarding_work_is_asked_twice() {
        let (home, repo) = crate::worktree::testing::repo("panes-leave");
        let repo = crate::worktree::repo_of(&repo).unwrap().unwrap();
        let tree = crate::worktree::add(&home, &repo, "glut/pane-2").unwrap();
        crate::worktree::testing::commit(&tree.place, "b.txt", "two\n");
        let (mut panes, _inlets) = split(Side::Right);
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
        assert!(parse_send("").is_err() && parse_send("2").is_err());
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
        let (mut panes, _inlets) = split(Side::Right);
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
        let (mut panes, _inlets) = split(Side::Right);
        panes.active = 0;
        panes.active().pane_request = Some(parse_send("3 files changed").unwrap());
        panes.settle(&(Arc::new(crate::Standalone) as Arc<dyn Host>));
        assert_eq!(owners(&panes.list[1].app), ["From pane 1: 3 files changed"]);
    }

    fn told(panes: &mut Panes, from: u64, to: Option<&str>, text: &str) -> Result<String, String> {
        let (reply, mut answer) = oneshot::channel();
        panes.on_report(
            from,
            Report::Tell {
                to: to.map(str::to_owned),
                text: text.into(),
                reply,
            },
        );
        answer.try_recv().unwrap()
    }

    #[tokio::test]
    async fn a_models_message_is_no_word_of_the_owners_and_a_long_chain_waits_for_one() {
        let (mut panes, _inlets) = split(Side::Right);
        panes.sync();
        let (first, second) = (panes.list[0].id, panes.list[1].id);
        let said = told(&mut panes, first, None, "which test fails?").unwrap();
        assert!(said.contains("delivered: pane 2"), "{said}");
        let target = &panes.list[1].app;
        assert!(target.transcript.blocks.iter().any(|block| matches!(
            &block.kind,
            crate::transcript::Kind::Note { text, tone: Tone::Heed }
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
        panes.active().pane_request = Some(parse_split("d as nobody here").unwrap());
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
                place: Place::Ask
            }
        ));
        let Request::Background { name, role, place } =
            parse_background("critic as reviewer tree fix-x").unwrap()
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
        assert!(panes.list[0].app.background[0].starts_with("critic · all-rounder · rests"));
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
            id: 3,
            number: 3,
            side: Side::Right,
            resume: false,
            tree: None,
            role: None,
            background: Some("tester".to_owned()),
            reports: sender,
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
            .filter(|block| matches!(&block.kind, crate::transcript::Kind::Owner(text) if text.ends_with("update plan")))
            .count();
        assert_eq!(owners, 1);
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
        let (mut panes, _inlets) = split(Side::Right);
        let (sender, _) = mpsc::unbounded_channel();
        panes.building = Some(Building {
            id: 3,
            number: 3,
            side: Side::Right,
            resume: false,
            tree: None,
            role: None,
            background: None,
            reports: sender,
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
    async fn a_host_that_keeps_one_pane_is_not_split() {
        let (first, reports) = app();
        let (mut panes, _inlets) = Panes::new(first, reports, false, Servers::Owner);
        press(&mut panes, KeyCode::Char('x'), KeyModifiers::CONTROL);
        press(&mut panes, KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(panes.building.is_none() && panes.list.len() == 1);
    }

    #[tokio::test]
    async fn each_pane_has_its_footer_with_its_number_and_the_cursor_is_the_active_ones() {
        let (mut panes, _inlets) = split(Side::Right);
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
    fn closing_a_pane_shares_its_room_evenly_again() {
        // 1 | (2 over (3 | 4)): the inner pair takes a quarter each, so all four need 4 × 60 and three columns.
        let mut layout = Node::Pane(1);
        assert!(layout.split(1, 2, Side::Right));
        assert!(layout.split(2, 3, Side::Down));
        assert!(layout.split(3, 4, Side::Right));
        assert_eq!(layout.needs(60), 243);
        layout.remove(2);
        assert_eq!(
            layout,
            Node::Split {
                stacked: false,
                parts: vec![Node::Pane(1), Node::Pane(3), Node::Pane(4)],
            },
            "one row of three, not a pair inside a pair"
        );
        assert_eq!(layout.needs(60), 182);
        layout.remove(3);
        layout.remove(4);
        assert_eq!(layout, Node::Pane(1));
    }

    #[test]
    fn a_side_is_a_letter_or_its_word() {
        assert_eq!(Side::parse("l"), Some(Side::Left));
        assert_eq!(Side::parse("down"), Some(Side::Down));
        assert_eq!(Side::parse("x"), None);
        assert!(Side::Up.stacked() && Side::Up.before());
        assert!(!Side::Right.stacked() && !Side::Right.before());
    }

    #[test]
    fn numbers_read_as_a_list() {
        assert_eq!(numbers(&[2]), "2");
        assert_eq!(numbers(&[2, 3]), "2 and 3");
        assert_eq!(numbers(&[1, 2, 3]), "1, 2 and 3");
    }
}
