//! A list to pick from, filtered as the owner types (the model picker, `/provider`, `/effort`, `/editor`). The model
//! picker also sorts — by name, newest, price or window, whatever the provider — and can show only the free models
//! (T31); what an account may not use stands greyed below the rest.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glut::Effort;

use crate::finder::Corpus;

/// What picking an entry does.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Model {
        provider: String,
        model: String,
    },
    Provider(String),
    /// `/provider key`'s question after a new key did not check out (T70): keep it anyway (`true`) or the old one.
    KeyKeep(bool),
    Effort(Option<Effort>),
    Editor(String),
    Session(String),
    /// A file of the folder to mention after `@` (T9).
    File(String),
    /// A theme by its name (`/theme`).
    Theme(String),
    /// A language for Glut to speak (T48).
    Language(glut::lang::Lang),
    /// Rewind (Esc Esc): the owner's message to go back to before, by its mark.
    RewindTo(usize),
    /// Fork (T14): a new session from before the owner's message `mark`, this one kept as it is.
    Fork(usize),
    /// A snapshot step of a run, for `/steps`: its run and turn.
    Step {
        run: String,
        turn: u32,
    },
    /// What to do with a step: show its change, or put the files back to before it.
    StepDo {
        run: String,
        turn: u32,
        restore: bool,
    },
    /// `/gate`: take back what an "always" answer allowed, by the rules it became.
    TakeBack(Vec<String>),
    /// An entry only to be read (a rule of the settings in `/gate`): picking it closes the list.
    Shown,
    /// `/resume`'s ⌫, asked once (T45): the kept session of this id deleted, its images no session needs with it.
    DeleteSession(String),
    /// `/memory` (T35): a fact to show, by its name.
    MemoryFact(String),
    /// `/todo clean` (T41): the owner's yes to dropping the done items.
    TodoClean,
    /// `/skill` (T37c): a skill to start, by its name.
    Skill(String),
    /// `/close`'s list: the pane or agent of this session id to close.
    ClosePane(u64),
    /// `/sessions` (T83): a pane of another Glut window, `/send <address> ` put into the input.
    WriteTo(String),
    /// `/login` (PV4): a subscription to sign in to, or out of, by its provider id.
    Login(String),
    /// `/login`'s second ⏎ on a signed-in subscription: signed out, revoked at its provider.
    Logout(String),
    /// `glut_file`'s `pick_model` (T37c): the model the owner picks for an agent's or a skill's own — never a switch
    /// of the session's, and no other list's pick ever answers it.
    OwnModel {
        provider: String,
        model: String,
    },
    /// `/init` (T35): whether `CLAUDE.md` moves into `AGENTS.md` (`true`) or stays the guide (`false`).
    Init(bool),
    /// The owner's message held what looks like a key (T58): sent masked (`true`) or as it is.
    SendSecrets(bool),
    /// What a pane asks of the screen, picked (T36a): where a new pane works, how a worktree's pane ends.
    Pane(crate::panes::Request),
    /// A command put into the input for the owner to change before Enter (a new worktree's branch name).
    Input(String),
    /// `/role` (T36c): the role this pane plays from now on; `None` an all-rounder.
    Role(Option<String>),
    /// `/agents`: a subagent's view, by its path through the streams.
    Agent(Vec<usize>),
    /// Rewind: how far — the conversation, the files, or both.
    Rewind {
        mark: usize,
        conversation: bool,
        files: bool,
    },
    /// `/projects` (T44): the folder of a project to switch to.
    Project(std::path::PathBuf),
    /// `/resume`'s search (T55): a session found in its conversation or among every project's, by its id and folder,
    /// and what the stream's search looks for once it is open (nothing for a title's hit).
    Found {
        id: String,
        cwd: std::path::PathBuf,
        find: String,
    },
}

/// What a model entry is sorted and filtered by.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Facts {
    pub created: Option<i64>,
    /// Input and output price per million tokens, added up.
    pub price: Option<f64>,
    pub window: Option<u32>,
    pub free: bool,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    /// A quieter second column: window and price, a command line.
    pub detail: String,
    pub pick: Pick,
    /// The account may not use it (OpenRouter's guardrails): shown greyed, below the rest.
    pub dim: bool,
    pub facts: Facts,
}

impl Item {
    pub fn plain(label: impl Into<String>, detail: impl Into<String>, pick: Pick) -> Self {
        Item {
            label: label.into(),
            detail: detail.into(),
            pick,
            dim: false,
            facts: Facts::default(),
        }
    }
}

/// How the model picker orders its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Newest,
    Cheapest,
    Largest,
}

impl Sort {
    fn next(self) -> Self {
        match self {
            Sort::Name => Sort::Newest,
            Sort::Newest => Sort::Cheapest,
            Sort::Cheapest => Sort::Largest,
            Sort::Largest => Sort::Name,
        }
    }

    /// How it is said in the language read now (T48).
    pub fn label(self) -> &'static str {
        let at = match self {
            Sort::Name => 0,
            Sort::Newest => 1,
            Sort::Cheapest => 2,
            Sort::Largest => 3,
        };
        crate::texts::t()
            .sort_names
            .get(at)
            .copied()
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct Picker {
    pub title: String,
    items: Vec<Item>,
    pub query: String,
    /// The selected entry among the shown ones.
    pub selected: usize,
    /// `Some` for a picker that sorts (the models).
    pub sort: Option<Sort>,
    pub free_only: bool,
    /// The list `glut_file`'s `pick_model` opened (T37c): its own, even with no entry to show.
    pub asks_model: bool,
    /// A list Glut opened by itself whose pick does something hard to take back (T36a: a merge, a discard): no key
    /// typed before it showed picks, a held key picks nothing, and typing does not filter it.
    pub guarded: bool,
    /// When it came up — or came into view again with its pane.
    pub opened: std::time::Instant,
    /// A list whose letters are keys (`/projects`: `p` pins, `x` hides): typing does not filter it.
    pub keyed: bool,
    /// `/resume` (T55): what it finds in the conversations, below the entries that fit.
    pub finding: Option<Finding>,
}

/// The kept session an entry opens, if it opens one.
fn session_of(item: &Item) -> Option<&str> {
    match &item.pick {
        Pick::Session(id) => Some(id.as_str()),
        _ => None,
    }
}

/// What `/resume` finds in the kept conversations as the owner types (T55).
#[derive(Debug)]
pub struct Finding {
    /// The sessions read beside the screen; `None` until they came.
    corpus: Option<Arc<Corpus>>,
    /// Every project's sessions (⌃A), not only the folder's.
    pub all: bool,
    cwd: PathBuf,
    /// The rows found for the query, below the entries that fit it.
    rows: Vec<Item>,
}

impl Finding {
    /// Whether the sessions are still being read.
    pub fn reading(&self) -> bool {
        self.corpus.is_none()
    }
}

impl Picker {
    /// A picker over `items`, the entry `current` selected where it is there.
    pub fn new(title: impl Into<String>, items: Vec<Item>, current: Option<&Pick>) -> Self {
        let mut picker = Picker {
            title: title.into(),
            items,
            query: String::new(),
            selected: 0,
            sort: None,
            free_only: false,
            asks_model: false,
            guarded: false,
            opened: std::time::Instant::now(),
            keyed: false,
            finding: None,
        };
        picker.select(current);
        picker
    }

    /// Its entries, taken out (to build another list of them).
    pub fn into_items(self) -> Vec<Item> {
        self.items
    }

    /// A picker of Glut's own whose pick is hard to take back: see [`Picker::guarded`]'s field.
    pub fn guarded(mut self) -> Self {
        self.guarded = true;
        self
    }

    /// A picker whose letters are keys: see [`Picker::keyed`]'s field.
    pub fn keyed(mut self) -> Self {
        self.keyed = true;
        self
    }

    /// A picker that also finds in the kept conversations of `cwd` (T55), once [`Picker::found`] gives them.
    pub fn finding(mut self, cwd: &Path) -> Self {
        self.finding = Some(Finding {
            corpus: None,
            all: false,
            cwd: cwd.to_path_buf(),
            rows: Vec::new(),
        });
        self
    }

    /// The kept conversations read beside the screen came: what was typed meanwhile is looked for at once.
    pub fn found(&mut self, corpus: Arc<Corpus>) {
        if let Some(finding) = &mut self.finding {
            finding.corpus = Some(corpus);
            self.refind();
        }
    }

    /// Every project's sessions, or the folder's again (⌃A).
    pub fn toggle_all(&mut self) {
        if let Some(finding) = &mut self.finding {
            finding.all = !finding.all;
            self.selected = 0;
            self.refind();
        }
    }

    /// The query as typed elsewhere (`/search <text>`).
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
        self.selected = 0;
        self.refind();
    }

    /// The rows found for the query anew: the sessions the list shows itself are left to it — every project's are
    /// all rows.
    fn refind(&mut self) {
        let Some(Finding {
            corpus: Some(corpus),
            all,
            cwd,
            ..
        }) = &self.finding
        else {
            return;
        };
        let (listed, shown): (Vec<&str>, Vec<&str>) = if *all {
            (Vec::new(), Vec::new())
        } else {
            (
                self.items.iter().filter_map(session_of).collect(),
                self.fitting().into_iter().filter_map(session_of).collect(),
            )
        };
        let hits = corpus.find(
            &self.query,
            (!*all).then_some(cwd.as_path()),
            &listed,
            &shown,
        );
        let rows = hits
            .into_iter()
            .map(|hit| {
                let title = match crate::transcript::clean(&hit.title) {
                    title if title.trim().is_empty() => format!("({})", tr!(no_message)),
                    title => title,
                };
                let label = if *all {
                    format!("{title} · {}", crate::finder::project_of(&hit.cwd))
                } else {
                    title
                };
                let when = hit
                    .updated
                    .with_timezone(&chrono::Local)
                    .format(glut::lang::date_time())
                    .to_string();
                let detail = match (&hit.snippet, hit.more) {
                    (None, _) => when,
                    (Some(snippet), 0) => snippet.clone(),
                    (Some(snippet), more) => {
                        format!("{snippet}  {}", tr!(found_more, count = more))
                    }
                };
                Item::plain(
                    label,
                    detail,
                    Pick::Found {
                        id: hit.id,
                        cwd: hit.cwd,
                        find: if hit.snippet.is_some() {
                            hit.find
                        } else {
                            String::new()
                        },
                    },
                )
            })
            .collect();
        if let Some(finding) = &mut self.finding {
            finding.rows = rows;
        }
    }

    /// A picker that sorts and filters its models.
    pub fn sorting(mut self) -> Self {
        let current = self.chosen();
        self.sort = Some(Sort::Name);
        self.select(current.as_ref());
        self
    }

    fn select(&mut self, current: Option<&Pick>) {
        self.selected = current
            .and_then(|current| self.shown().iter().position(|item| &item.pick == current))
            .unwrap_or(0);
    }

    /// The entries whose label or detail holds every word typed, in any case — the usable ones first, in the
    /// order chosen —, and below them what `/resume` found in the conversations (every project's alone with ⌃A).
    pub fn shown(&self) -> Vec<&Item> {
        let mut shown = match &self.finding {
            Some(finding) if finding.all => Vec::new(),
            _ => self.fitting(),
        };
        if let Some(finding) = &self.finding {
            shown.extend(finding.rows.iter());
        }
        shown
    }

    /// The entries that fit what was typed, as [`Picker::shown`] lists them first.
    fn fitting(&self) -> Vec<&Item> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut shown: Vec<&Item> = self
            .items
            .iter()
            .filter(|item| !self.free_only || item.facts.free)
            .filter(|item| {
                let text = format!("{} {}", item.label, item.detail).to_lowercase();
                words.iter().all(|word| text.contains(word.as_str()))
            })
            .collect();
        if let Some(sort) = self.sort {
            shown.sort_by(|a, b| {
                a.dim.cmp(&b.dim).then_with(|| match sort {
                    Sort::Name => a.label.cmp(&b.label),
                    // `None` sorts below any `Some` here: what the list does not say comes last.
                    Sort::Newest => b.facts.created.cmp(&a.facts.created),
                    Sort::Cheapest => {
                        let price = |item: &Item| item.facts.price.unwrap_or(f64::INFINITY);
                        price(a)
                            .total_cmp(&price(b))
                            .then_with(|| a.label.cmp(&b.label))
                    }
                    Sort::Largest => b.facts.window.cmp(&a.facts.window),
                })
            });
        }
        shown
    }

    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.refind();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.refind();
    }

    pub fn up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn down(&mut self) {
        let count = self.shown().len();
        if self.selected + 1 < count {
            self.selected += 1;
        }
    }

    /// The next order (Ctrl+S), the chosen entry kept chosen.
    pub fn next_sort(&mut self) {
        if let Some(sort) = self.sort {
            let current = self.chosen();
            self.sort = Some(sort.next());
            self.select(current.as_ref());
        }
    }

    /// Only the free models, or all again (Ctrl+F).
    pub fn toggle_free(&mut self) {
        if self.sort.is_some() {
            self.free_only = !self.free_only;
            self.selected = 0;
        }
    }

    /// The selected entry's pick.
    pub fn chosen(&self) -> Option<Pick> {
        self.shown()
            .get(self.selected)
            .map(|item| item.pick.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str) -> Item {
        Item::plain(label, "", Pick::Provider(label.into()))
    }

    fn model(label: &str, created: i64, price: f64, dim: bool) -> Item {
        Item {
            dim,
            facts: Facts {
                created: Some(created),
                price: Some(price),
                window: Some(created as u32),
                free: price == 0.0,
            },
            ..item(label)
        }
    }

    fn labels(picker: &Picker) -> Vec<&str> {
        picker
            .shown()
            .iter()
            .map(|item| item.label.as_str())
            .collect()
    }

    #[test]
    fn typing_filters_by_every_word_and_the_selection_stays_inside() {
        let mut picker = Picker::new(
            "Provider",
            vec![item("openrouter"), item("ollama"), item("open-box")],
            Some(&Pick::Provider("ollama".into())),
        );
        assert_eq!(picker.selected, 1, "the current one is selected");
        picker.type_char('o');
        picker.type_char('p');
        assert_eq!(picker.shown().len(), 2);
        picker.down();
        picker.down();
        assert_eq!(
            picker.chosen(),
            Some(Pick::Provider("open-box".into())),
            "the last stays selected"
        );
        picker.query = "nothing here".into();
        assert_eq!(picker.chosen(), None);
    }

    #[test]
    fn models_the_account_may_use_come_first_in_every_order_and_free_ones_can_stand_alone() {
        let mut picker = Picker::new(
            "Model",
            vec![
                model("b-old", 1, 2.0, false),
                model("z-blocked-new", 9, 0.5, true),
                model("a-new", 5, 0.0, false),
            ],
            Some(&Pick::Provider("b-old".into())),
        )
        .sorting();
        assert_eq!(labels(&picker), ["a-new", "b-old", "z-blocked-new"]);
        assert_eq!(
            picker.chosen(),
            Some(Pick::Provider("b-old".into())),
            "the current one stays chosen"
        );
        picker.next_sort();
        assert_eq!(picker.sort, Some(Sort::Newest));
        assert_eq!(
            labels(&picker),
            ["a-new", "b-old", "z-blocked-new"],
            "blocked stays below, however new"
        );
        assert_eq!(picker.chosen(), Some(Pick::Provider("b-old".into())));
        picker.next_sort();
        assert_eq!(
            labels(&picker),
            ["a-new", "b-old", "z-blocked-new"],
            "cheapest first"
        );
        picker.toggle_free();
        assert_eq!(labels(&picker), ["a-new"]);
    }
}
