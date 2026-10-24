//! Glut's terminal interface (`docs/plans/glut-tui.md`): a full-screen session over the glut harness — run by the
//! standalone `glut`, and by a host that runs Glut for its own ends (TUI8a): [`run`] takes the terminal for one
//! session built as a [`Launch`] says, with a [`Host`] taking part in it.

mod agents;
mod app;
mod branch;
mod commands;
mod follow;
mod footer;
mod glimm;
mod guides;
mod highlight;
pub mod host;
mod input;
pub mod log;
mod markdown;
mod memory;
mod meter;
pub mod motion;
mod panes;
mod picker;
mod plan;
mod prefs;
mod review;
mod screen;
mod scrollback;
mod select;
mod session;
pub mod setup;
mod shell;
mod store;
mod tell;
mod theme;
mod todos;
mod transcript;
mod vi;
mod view;
mod warm;
mod watch;
mod worktree;

pub use follow::{resolve as watched_log, watch};
pub use host::{
    EngineChoice, Host, HostFuture, HostLink, Instructions, Launch, Log, Search, Servers, Signal,
    Standalone, State, Turn,
};
pub use screen::{Open, run, standalone};
