//! Español.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "idioma: {name}",
    language_unkept: "idioma: {name} — solo para esta ejecución: {reason}",
    language_unknown: "no hay idioma {name} — {known}",
    language_title: "Idioma de Glut",
    language_now: "actual",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
};
