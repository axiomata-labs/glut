//! Português (Brasil).

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "idioma: {name}",
    language_unkept: "idioma: {name} — só nesta execução: {reason}",
    language_unknown: "nenhum idioma {name} — {known}",
    language_title: "Idioma do Glut",
    language_now: "atual",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
};
