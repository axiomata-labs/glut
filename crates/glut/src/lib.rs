//! Glut's agent harness, without its terminal UI.
//!
//! A lean agent loop: a model behind an OpenAI-compatible chat API is given a small system prompt, the caller's
//! instructions and a set of tools, and the loop runs the tool calls it asks for until it answers without one. Every
//! step is reported as an [`Event`], so a caller can show it, log it or write it out as JSON lines (the `glut` binary).
//!
//! Which model to use, the key, the folder to work in and the instructions come from the caller as plain values. File
//! access goes through `axiomata-files`' guarded [`axiomata_files::Root`], never through a raw path.

pub mod agents;
pub mod anthropic;
pub mod classifier;
pub mod clean;
pub mod cli;
pub mod codemode;
pub mod commands;
pub mod compact;
pub mod config;
pub(crate) mod diagnostics;
pub mod editor;
pub mod error;
pub mod event;
pub mod folders;
pub mod guide;
pub mod hooks;
pub mod images;
pub mod journal;
pub mod keychain;
pub mod lang;
pub mod lean;
mod lsp;
pub mod masks;
pub mod mcp;
pub mod model;
pub mod oauth;
pub mod permissions;
pub mod planning;
pub mod project;
pub mod prompt;
pub mod providers;
mod refusals;
pub mod responses;
pub mod results;
pub mod run;
pub mod sandbox;
pub mod setup;
pub mod skills;
pub mod snapshot;
pub mod subscription;
mod task;
pub mod team;
pub mod tools;

pub use config::{
    Budget, Compaction, Effort, Inbox, LOOP_TOOL, ModelEndpoint, PANE_TOOLS, Prices, RefusalLimits,
    RunConfig, TEAM_TOOLS,
};
pub use diagnostics::{LanguageServers, ServerState};
pub use error::HarnessError;
pub use event::{Event, EventSink, FinishReason, McpStatus, NudgeKind, SharedSink, SkillHow};
pub use model::{ChatModel, Message, Model, OpenAiCompat, Piece, Reply, ToolCall, ToolSpec, Usage};
pub use permissions::{Access, Gate, Mode, Rule, RuleSet};
pub use run::{Ahead, Cancel, RunOutcome, Warm, compact_from, resume, run, run_continuing};
pub use tools::{Tool, ToolFuture, Toolbox};
