//! 日本語.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "言語: {name}",
    language_unkept: "言語: {name} — 今回の実行のみ: {reason}",
    language_unknown: "言語 {name} はありません — {known}",
    language_title: "Glut の言語",
    language_now: "現在",
    panes_count: Plural {
        one: "{n} 個の Pane",
        other: "{n} 個の Pane",
    },
};
