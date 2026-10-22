//! Glut's terminal interface (`docs/plans/glut-tui.md`): a full-screen session over the glut harness — run by the
//! standalone `glut`, and by a host that runs Glut for its own ends (TUI8a): [`run`] takes the terminal for one
//! session built as a [`Launch`] says, with a [`Host`] taking part in it.

mod agents;
mod app;
mod glimm;
mod highlight;
pub mod host;
mod input;
mod markdown;
mod meter;
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
mod theme;
mod transcript;
mod vi;
mod view;
mod watch;

pub use host::{
    EngineChoice, Host, HostFuture, HostLink, Instructions, Launch, Servers, Signal, Standalone,
    State,
};
pub use screen::{Open, run};
