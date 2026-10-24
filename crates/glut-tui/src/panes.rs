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

/// The most panes for now (T36a): two side by side, or one above the other.
pub const MOST: usize = 2;
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
    /// A new pane at `side` working at `place`; `resume`: an earlier session in it, picked from the list.
    Split {
        side: Side,
        resume: bool,
        place: Place,
    },
    /// Close the asking pane.
    Close,
    /// Close the asking pane, its worktree ending so.
    CloseAs(Leaving),
    /// The keys to the other pane.
    Toggle,
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
    let mut words = rest.split_whitespace().peekable();
    while let Some(word) = words.next() {
        match (word, Side::parse(word)) {
            (_, Some(given)) => side = given,
            ("resume", None) => resume = true,
            ("here", None) => place = Place::Here,
            ("tree", None) => {
                let branch = words.next_if(|next| {
                    Side::parse(next).is_none() && !matches!(*next, "resume" | "here" | "tree")
                });
                place = Place::Tree(branch.unwrap_or_default().to_owned());
            }
            (other, None) => {
                return Err(format!(
                    "/split {other}: l, r, u or d; resume; here or tree [branch]"
                ));
            }
        }
    }
    Ok(Request::Split {
        side,
        resume,
        place,
    })
}

/// One pane: its number and its session.
pub struct Pane {
    /// Its session's own mark on the reports, never given twice — unlike the number a closed pane leaves free: a late
    /// report of a closed session must reach no pane.
    id: u64,
    /// The number the owner sees: the lowest free one when it opened.
    pub number: u8,
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
    reports: mpsc::UnboundedSender<Report>,
}

/// A new pane's session as `session::build` makes it.
type Built = Result<(session::Parts, session::Engine), String>;

/// The panes on the screen.
pub struct Panes {
    /// The panes in the order they stand: left to right, or top to bottom.
    pub list: Vec<Pane>,
    /// The pane the keys go to, as a place in `list`.
    pub active: usize,
    /// One above the other rather than side by side.
    pub stacked: bool,
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
                app,
                area: Rect::default(),
                tree: None,
            }],
            active: 0,
            stacked: false,
            may_split,
            servers,
            reports,
            next_id: 2,
            building: None,
            built,
            quit_armed: None,
            close_armed: None,
            redraw: false,
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

    /// Whether more than one pane is open.
    pub fn split(&self) -> bool {
        self.list.len() > 1
    }

    /// Once per frame: answered questions go, and what the panes share is told to each — the terminal's mouse (the
    /// active pane's word is the screen's) and the sessions the others hold.
    pub fn sync(&mut self) {
        let held = self.list[self.active].app.mouse_held;
        let open: Vec<String> = self
            .list
            .iter()
            .map(|pane| pane.app.session.id.clone())
            .collect();
        for (at, pane) in self.list.iter_mut().enumerate() {
            pane.app.drop_answered_ask();
            pane.app.mouse_held = held;
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
        if let Some(pane) = self.list.iter_mut().find(|pane| pane.id == id) {
            pane.app.on_report(report);
        }
    }

    /// A key, a paste or the mouse: the active pane's, but a click moves into the pane under it (the click itself
    /// goes nowhere) and the wheel goes to the pane under it.
    pub fn on_terminal(&mut self, event: TermEvent) {
        if let TermEvent::Mouse(mouse) = &event
            && self.split()
        {
            let under = self
                .list
                .iter()
                .position(|pane| contains(pane.area, mouse.column, mouse.row));
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
            if at != self.active {
                self.focus(at);
            }
            match request {
                Request::Toggle if !self.split() => {
                    self.active().say("one pane — /split splits the window");
                }
                Request::Toggle => self.focus((self.active + 1) % self.list.len()),
                Request::Close => self.close(),
                Request::CloseAs(leaving) => self.leave(leaving),
                Request::Split {
                    side,
                    resume,
                    place,
                } => self.start(side, resume, place, host),
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
        let working: Vec<u8> = self
            .list
            .iter()
            .enumerate()
            .filter(|(other, pane)| *other != at && pane.app.working())
            .map(|(_, pane)| pane.number)
            .collect();
        let armed = self.quit_armed.is_some_and(|since| since.elapsed() < ARMED);
        if working.is_empty() || armed {
            return true;
        }
        self.quit_armed = Some(Instant::now());
        let app = &mut self.list[at].app;
        app.quit = false;
        app.say(format!(
            "pane {} still works — /quit again ends it too",
            numbers(&working)
        ));
        false
    }

    /// Closes the active pane; a turn under way is stopped — said once first. The last pane is not closed.
    fn close(&mut self) {
        if !self.split() {
            return self.active().say("the last pane — /quit ends Glut");
        }
        let number = self.list[self.active].number;
        let armed = self
            .close_armed
            .is_some_and(|(armed, since)| armed == number && since.elapsed() < ARMED);
        if self.active().working() && !armed {
            self.close_armed = Some((number, Instant::now()));
            return self
                .active()
                .say("this pane works — /close again stops it and closes the pane");
        }
        self.close_armed = None;
        if let Some(tree) = self.list[self.active].tree.clone() {
            // Git goes over the worktree only once nothing writes there any more.
            if self.active().working() {
                self.active().stop_turn();
                return self
                    .active()
                    .say("its turn is stopping — /close again once it has stopped, to say how the worktree ends");
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
                .say("this pane works again — /close once it is done");
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
                self.active().say(said);
            }
            Err(reason) => self.active().transcript.note(reason, Tone::Fault),
        }
    }

    /// Takes the active pane away; a turn under way is stopped.
    fn remove_active(&mut self) {
        let mut pane = self.list.remove(self.active);
        let number = pane.number;
        let stopped = pane.app.working();
        pane.app.stop_turn();
        self.focus(self.active.min(self.list.len() - 1));
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
        self.active().say(format!("pane {number} closed — {kept}"));
    }

    /// Starts a new pane beside the active one: a session in the same folder, on the active pane's provider and
    /// model, in build — built beside the screen.
    fn start(&mut self, side: Side, resume: bool, place: Place, host: &Arc<dyn Host>) {
        if !self.may_split {
            return self
                .active()
                .say("this session's host keeps one pane: its studio splits its own windows");
        }
        if self.list.len() >= MOST {
            return self.active().say(format!("at most {MOST} panes for now"));
        }
        if self.building.is_some() {
            return self.active().say("a pane is starting already");
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
                self.active().say(format!("no worktree offered: {reason}"));
                None
            }
            (Place::Ask, Ok(Some(repo))) => {
                let picker = where_picker(side, resume, &repo, number);
                self.active().picker = Some(picker);
                return;
            }
            (Place::Tree(_), Err(reason)) => {
                return self.active().transcript.note(reason, Tone::Fault);
            }
            (Place::Tree(_), Ok(None)) => {
                return self
                    .active()
                    .say("this folder is in no git repository: /split here");
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
        app.say(format!("pane {number} starts…"));
        let (reports, inlet) = mpsc::unbounded_channel();
        forward(id, inlet, self.reports.clone());
        self.building = Some(Building {
            id,
            number,
            side,
            resume,
            tree,
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
        // Two panes leave little room beside them: the side panel starts hidden (⌃B brings it, where there is room).
        app.panel = Some(false);
        let current = &mut self.list[self.active].app;
        current.panel = Some(false);
        let busy_here =
            !current.input.is_empty() || current.waits_for_owner() || current.picker.is_some();
        let at = if building.side.before() {
            self.active
        } else {
            self.active + 1
        };
        self.stacked = building.side.stacked();
        self.list.insert(
            at,
            Pane {
                id: building.id,
                number: building.number,
                app,
                area: Rect::default(),
                tree: building.tree,
            },
        );
        if busy_here {
            // The active pane moved one place on when the new one went before it.
            if at <= self.active {
                self.active += 1;
            }
            let number = building.number;
            self.active()
                .say(format!("pane {number} is ready — ⌃P goes there"));
        } else {
            self.focus(at);
        }
    }

    /// Moves the keys to pane `at`; what it asks counts as shown from now.
    fn focus(&mut self, at: usize) {
        self.active = at;
        self.active().shown_again();
    }

    /// The panes' areas in `area`: side by side with a column between them, or one above the other.
    pub fn areas(&self, area: Rect) -> Vec<Rect> {
        let count = self.list.len() as u16;
        if count <= 1 {
            return vec![area];
        }
        if self.stacked {
            let height = area.height / count;
            (0..count)
                .map(|at| Rect {
                    y: area.y + at * height,
                    height: if at + 1 == count {
                        area.height - at * height
                    } else {
                        height
                    },
                    ..area
                })
                .collect()
        } else {
            // One column between each two panes for the line that parts them.
            let width = (area.width.saturating_sub(count - 1)) / count;
            (0..count)
                .map(|at| Rect {
                    x: area.x + at * (width + 1),
                    width: if at + 1 == count {
                        area.width.saturating_sub(at * (width + 1))
                    } else {
                        width
                    },
                    ..area
                })
                .collect()
        }
    }
}

/// Where a new pane in a git repository works: this folder, or a worktree whose branch the owner may rename first
/// (the command goes into the input).
fn where_picker(
    side: Side,
    resume: bool,
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
    let items = vec![
        Item::plain(
            "this folder",
            "both panes change the same files",
            Pick::Pane(Request::Split {
                side,
                resume,
                place: Place::Here,
            }),
        ),
        Item::plain(
            "a new worktree",
            format!("on the branch {branch} — ⏎ puts it in the input to rename first"),
            Pick::Input(format!("/split {letter}{resume_word} tree {branch}")),
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
fn numbers(numbers: &[u8]) -> String {
    let words: Vec<String> = numbers.iter().map(u8::to_string).collect();
    match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => words.join(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

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
            (numbers_of(&panes), panes.active, panes.stacked),
            (vec![1, 2], 1, false)
        );
        let (panes, _inlets) = split(Side::Left);
        assert_eq!(
            (numbers_of(&panes), panes.active),
            (vec![2, 1], 0),
            "2 stands left"
        );
        let (panes, _inlets) = split(Side::Down);
        assert!(panes.stacked);
        let areas = panes.areas(Rect::new(0, 0, 100, 41));
        assert_eq!((areas[0].height, areas[1].y, areas[1].height), (20, 20, 21));
        let (panes, _inlets) = split(Side::Right);
        let areas = panes.areas(Rect::new(0, 0, 101, 40));
        assert_eq!(
            (areas[0].width, areas[1].x, areas[1].width),
            (50, 51, 50),
            "a column parts them"
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
                place: Place::Ask
            }
        );
        assert_eq!(
            parse_split("d resume here").unwrap(),
            Request::Split {
                side: Side::Down,
                resume: true,
                place: Place::Here
            }
        );
        assert_eq!(
            parse_split("tree glut/plan r").unwrap(),
            Request::Split {
                side: Side::Right,
                resume: false,
                place: Place::Tree("glut/plan".into())
            }
        );
        assert_eq!(
            parse_split("l tree").unwrap(),
            Request::Split {
                side: Side::Left,
                resume: false,
                place: Place::Tree(String::new())
            }
        );
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
