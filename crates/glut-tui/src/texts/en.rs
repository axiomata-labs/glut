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
    topics: &[
        "Session",
        "Model",
        "View",
        "Panes",
        "Changes",
        "Project",
        "Settings",
        "Input",
        "Editing the line",
        "Help",
    ],
    // The commands' and keys' own words are English in `commands.rs`: the source, read where a language has none.
    commands: &[],
    keys: &[],
    help_commands: "Commands",
    help_keys: "Keys ({legend}; ⌃K keeps them in the side panel)",
    help_also: "Also: !cmd runs a command and shows Glut its output · !!cmd runs it for you alone · a large paste \
                stands as [pasted N lines] · drag to copy · a click on a subagent's tile opens it",
    help_vim: "/vim on: ⎋ to Normal mode (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y with a motion or \
               doubled · counts · p P · u ⌃R), i a I A o O to Insert; ⎋ in Normal mode stops Glut",
    legend: "⌃ Ctrl · ⇧ Shift · ⌥ Alt · ⏎ Enter · ⇥ Tab · ⎋ Esc",
};
