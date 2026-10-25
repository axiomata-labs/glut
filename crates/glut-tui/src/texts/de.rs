//! Deutsch.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "Sprache: {name}",
    language_unkept: "Sprache: {name} — nur für diesen Lauf: {reason}",
    language_unknown: "keine Sprache {name} — {known}",
    language_title: "Sprache von Glut",
    language_now: "jetzt",
    panes_count: Plural {
        one: "{n} Pane",
        other: "{n} Panes",
    },
};
