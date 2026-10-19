//! Axiomata-OS's own agent harness (`docs/plans/harness.md`, board card #37).
//!
//! A lean agent loop: a model behind an OpenAI-compatible chat API is given a small system prompt, the caller's
//! instructions and a set of tools, and the loop runs the tool calls it asks for until it answers without one. Every
//! step is reported as an [`Event`], so a caller can show it, log it or write it out as JSON lines (the `axh` binary).
//!
//! The crate depends on nothing of Axiomata's core or its app (H2): which model to use, the key, the folder to work
//! in and the instructions come from the caller as plain values, so the harness can later stand on its own. File
//! access goes through `axiomata-files`' guarded [`axiomata_files::Root`], never through a raw path.

pub mod codemode;
pub mod commands;
pub mod config;
pub mod error;
pub mod event;
pub mod history;
pub mod hooks;
pub mod mcp;
pub mod model;
pub mod permissions;
pub mod planning;
pub mod prompt;
pub mod run;
pub mod sandbox;
pub mod setup;
pub mod skills;
mod task;
pub mod tools;

pub use config::{ModelEndpoint, RunConfig};
pub use error::HarnessError;
pub use event::{Event, EventSink, FinishReason, McpStatus, SharedSink, SkillHow};
pub use model::{ChatModel, Message, OpenAiCompat, Reply, ToolCall, ToolSpec, Usage};
pub use permissions::{Access, Gate, Mode, Rule, RuleSet};
pub use run::{Cancel, RunOutcome, run, run_continuing};
pub use tools::{Tool, ToolFuture, Toolbox};
