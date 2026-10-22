//! What can end a run with an error.

use thiserror::Error;

/// What a provider's answer means in a few words, where its own words leave the owner guessing.
fn hint(body: &str) -> &'static str {
    if body.contains("FreeTierError") {
        " — OpenCode's free models run only inside OpenCode itself: pick a paid one (/model)"
    } else if body.contains("credit balance is too low") {
        " — the API's credits are bought in the provider's console, apart from a subscription's"
    } else {
        ""
    }
}

/// A failure of the run itself. A tool that fails is not one of these: its error goes back to the model as text.
#[derive(Debug, Error)]
pub enum HarnessError {
    /// The model API answered with an error status; `body` is the start of its answer.
    #[error("the model API answered {status}: {body}{}", hint(body))]
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
    /// A skill cannot be started (HS6, K5): a `prepend_files` path that is absolute or climbs out of the working
    /// folder, or a file along the way that cannot be read.
    #[error("skill {name}: {message}")]
    Skill { name: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_providers_puzzling_refusal_gets_a_hint() {
        let free = HarnessError::Http {
            status: 403,
            body: r#"{"error":{"type":"FreeTierError","message":"…"}}"#.into(),
        };
        assert!(free.to_string().contains("pick a paid one"), "{free}");
        let plain = HarnessError::Http {
            status: 500,
            body: "boom".into(),
        };
        assert_eq!(plain.to_string(), "the model API answered 500: boom");
    }
}
