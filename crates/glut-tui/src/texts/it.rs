//! Italiano.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "lingua: {name}",
    language_unkept: "lingua: {name} — solo per questa esecuzione: {reason}",
    language_unknown: "nessuna lingua {name} — {known}",
    language_title: "Lingua di Glut",
    language_now: "attuale",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} pane",
    },
};
