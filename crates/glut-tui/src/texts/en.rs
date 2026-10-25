//! English, the source.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "language: {name}",
    language_unkept: "language: {name} — for this run only: {reason}",
    language_unknown: "no language {name} — {known}",
    language_title: "Glut's language",
    language_now: "now",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
};
