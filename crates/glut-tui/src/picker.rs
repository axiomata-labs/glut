//! A list to pick from, filtered as the owner types (the model picker, `/provider`, `/effort`, `/editor`). The model
//! picker also sorts — by name, newest, price or window, whatever the provider — and can show only the free models
//! (T31); what an account may not use stands greyed below the rest.

use glut::Effort;

/// What picking an entry does.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Model {
        provider: String,
        model: String,
    },
    Provider(String),
    Effort(Option<Effort>),
    Editor(String),
    Session(String),
    /// A session command by its name (the palette, T12).
    Run(String),
    /// The keys an action is bound to, pressed for the owner (the palette).
    Keys(Vec<crossterm::event::KeyEvent>),
    /// A file of the folder to mention after `@` (T9).
    File(String),
    /// `@` with the file picker, from the palette.
    Mention,
    /// A theme by its name (`/theme`).
    Theme(String),
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
    /// `/memory` (T35): a fact to show, by its name.
    MemoryFact(String),
    /// `/init` (T35): whether `CLAUDE.md` moves into `AGENTS.md` (`true`) or stays the guide (`false`).
    Init(bool),
    /// `/agents`: a subagent's view, by its path through the streams.
    Agent(Vec<usize>),
    /// Rewind: how far — the conversation, the files, or both.
    Rewind {
        mark: usize,
        conversation: bool,
        files: bool,
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

    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Newest => "newest",
            Sort::Cheapest => "cheapest",
            Sort::Largest => "largest window",
        }
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
        };
        picker.select(current);
        picker
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
    /// order chosen.
    pub fn shown(&self) -> Vec<&Item> {
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
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
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
