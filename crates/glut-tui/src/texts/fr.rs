//! Français.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "langue : {name}",
    language_unkept: "langue : {name} — pour cette exécution seulement : {reason}",
    language_unknown: "aucune langue {name} — {known}",
    language_title: "Langue de Glut",
    language_now: "actuelle",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
};
