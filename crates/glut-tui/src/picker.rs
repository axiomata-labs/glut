//! A list to pick from, filtered as the owner types (the model picker, `/provider`, `/effort`, `/editor`).

use glut::Effort;

/// What picking an entry does.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Model { provider: String, model: String },
    Provider(String),
    Effort(Option<Effort>),
    Editor(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    /// A quieter second column: window and price, a command line.
    pub detail: String,
    pub pick: Pick,
}

#[derive(Debug)]
pub struct Picker {
    pub title: String,
    items: Vec<Item>,
    pub query: String,
    /// The selected entry among the filtered ones.
    pub selected: usize,
}

impl Picker {
    /// A picker over `items`, the entry `current` selected where it is there.
    pub fn new(title: impl Into<String>, items: Vec<Item>, current: Option<&Pick>) -> Self {
        let selected = current
            .and_then(|current| items.iter().position(|item| &item.pick == current))
            .unwrap_or(0);
        Picker {
            title: title.into(),
            items,
            query: String::new(),
            selected,
        }
    }

    /// The entries whose label or detail holds every word typed, in any case.
    pub fn shown(&self) -> Vec<&Item> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.items
            .iter()
            .filter(|item| {
                let text = format!("{} {}", item.label, item.detail).to_lowercase();
                words.iter().all(|word| text.contains(word.as_str()))
            })
            .collect()
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
        Item {
            label: label.into(),
            detail: String::new(),
            pick: Pick::Provider(label.into()),
        }
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
}
