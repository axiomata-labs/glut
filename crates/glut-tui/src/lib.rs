//! Glut's terminal interface (`docs/plans/glut-tui.md`): a full-screen session over the glut harness — run by the
//! standalone `glut`, and by a host that runs Glut for its own ends (TUI8a): [`run`] takes the terminal for one
//! session built as a [`Launch`] says, with a [`Host`] taking part in it.

#[macro_use]
mod texts;
mod agents;
mod app;
mod background;
mod branch;
mod btw;
mod cli_help;
mod commands;
mod export;
mod finder;
mod follow;
mod footer;
mod glimm;
mod graphics;
mod guides;
mod helpbox;
mod highlight;
pub mod host;
mod input;
mod layout;
mod linebreak;
mod links;
pub mod log;
mod login;
mod looping;
mod manual;
mod markdown;
mod mcp;
mod memory;
mod meter;
pub mod motion;
mod notify;
mod panes;
mod paths;
mod picker;
mod pictures;
mod plan;
mod prefs;
mod projects;
mod review;
mod runs;
mod schedule;
mod scheduled;
mod screen;
mod scrollback;
mod select;
mod session;
pub mod setup;
mod shell;
mod store;
mod tell;
mod theme;
mod tiles;
mod todos;
mod transcript;
mod vi;
mod view;
mod warm;
mod watch;
mod worktree;

pub use cli_help::{localized as localized_cli, untranslated as untranslated_cli};
pub use export::{Format as ExportFormat, headless as export_headless};
pub use finder::headless as search_headless;
pub use follow::{resolve as watched_log, watch};
pub use host::{
    EngineChoice, Host, HostFuture, HostLink, Instructions, Launch, Log, Search, Servers, Signal,
    Standalone, State, Turn,
};
pub use panes::Switch;
pub use scheduled::{Command as ScheduleCommand, headless as schedule_headless};
pub use screen::{Open, restart_in, run, standalone, standalone_switching};
pub use texts::force as force_language;

/// The language a command starts in before any session does (`glut --help`, `glut watch`): the one `/language` kept,
/// else the system's — a `--lang` forced before wins (T48).
pub fn apply_kept_language() {
    let kept = glut::permissions::settings::home_dir()
        .ok()
        .and_then(|home| prefs::Prefs::load(&home).language);
    texts::apply(kept.as_deref(), None);
}
