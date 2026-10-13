//! What can end a run with an error.

use thiserror::Error;

/// A failure of the run itself. A tool that fails is not one of these: its error goes back to the model as text.
#[derive(Debug, Error)]
pub enum HarnessError {
    /// The model API answered with an error status; `body` is the start of its answer.
    #[error("the model API answered {status}: {body}")]
    Http { status: u16, body: String },
    /// The model API could not be reached or the connection broke.
    #[error("could not reach the model API: {0}")]
    Transport(String),
    /// The model API's answer was not what the protocol says.
    #[error("unexpected answer from the model API: {0}")]
    Protocol(String),
    /// A settings file (`~/.axh/settings.json`, a project's `.axh/`) cannot be used: unreadable, malformed, or holding
    /// a rule that does not parse. The run does not start without its rules.
    #[error("settings {path}: {reason}")]
    Settings { path: String, reason: String },
    /// The folder the run should work in cannot be used.
    #[error("cannot work in {path}: {reason}")]
    Workspace { path: String, reason: String },
    /// An MCP server could not be started or answered against the protocol (HS5); it is left out, the run goes on
    /// without it.
    #[error("MCP server {server}: {message}")]
    Mcp { server: String, message: String },
    /// An MCP server said nothing within the limit — a handshake that hangs, a call nobody answers.
    #[error("MCP server {server} did not answer within {timeout:?}")]
    McpTimeout {
        server: String,
        timeout: std::time::Duration,
    },
}
