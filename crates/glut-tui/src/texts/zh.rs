//! 简体中文.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "语言：{name}",
    language_unkept: "语言：{name} — 仅限本次运行：{reason}",
    language_unknown: "没有语言 {name} — {known}",
    language_title: "Glut 的语言",
    language_now: "当前",
    panes_count: Plural {
        one: "{n} 个 Pane",
        other: "{n} 个 Pane",
    },
};
