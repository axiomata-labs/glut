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
    /// The folder the run should work in cannot be used.
    #[error("cannot work in {path}: {reason}")]
    Workspace { path: String, reason: String },
}
