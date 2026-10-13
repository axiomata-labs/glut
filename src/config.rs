//! What a run is given: the model endpoint and the run's own settings.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// The most model turns a run takes before it stops (a turn is one model call). Generous for real work, but an end
/// to a model that keeps calling tools without ever answering.
pub const DEFAULT_MAX_TURNS: u32 = 40;

/// How long a whole run may take before it stops.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// A model behind an OpenAI-compatible chat API (`POST {base_url}/v1/chat/completions`): OpenRouter, Ollama and most
/// others speak it.
#[derive(Clone)]
pub struct ModelEndpoint {
    /// The API's base, without `/v1` (`https://openrouter.ai/api`, `http://localhost:11434`).
    pub base_url: String,
    /// Sent as a bearer token; `None` for an endpoint without one (a local Ollama).
    pub api_key: Option<String>,
    /// The model as the API names it (`deepseek/deepseek-chat`, `qwen3:8b`).
    pub model: String,
}

/// The key never shows in a log line or an error, whatever prints the endpoint.
impl fmt::Debug for ModelEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelEndpoint")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .finish()
    }
}

/// One run's settings.
#[derive(Debug, Clone)]
pub struct RunConfig {
    /// The folder the run works in; every file tool is confined to it.
    pub cwd: PathBuf,
    /// The caller's instructions, appended to the harness's own short system prompt (a role, a project's
    /// `AGENTS.md`, a skill's text).
    pub instructions: Option<String>,
    pub max_turns: u32,
    pub timeout: Duration,
    /// Passed to the model as `max_tokens`; `None` leaves it to the API.
    pub max_output_tokens: Option<u32>,
    /// More folders of skills beside the owner's `~/.axh/skills` and, in a trusted folder, the project's
    /// `.axh/skills` (HS6, K2): `<folder>/<name>/SKILL.md` each. `axh` fills them from its repeatable
    /// `--skill-dir`; a caller loads the catalog with them (`skills::Catalog::load`).
    pub skill_dirs: Vec<PathBuf>,
}

impl RunConfig {
    /// A run in `cwd` with the default limits and no instructions of the caller's.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        RunConfig {
            cwd: cwd.into(),
            instructions: None,
            max_turns: DEFAULT_MAX_TURNS,
            timeout: DEFAULT_TIMEOUT,
            max_output_tokens: None,
            skill_dirs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_never_printed() {
        let endpoint = ModelEndpoint {
            base_url: "https://example.test/api".into(),
            api_key: Some("sk-secret-123".into()),
            model: "m".into(),
        };
        let printed = format!("{endpoint:?}");
        assert!(!printed.contains("sk-secret-123"), "{printed}");
        assert!(printed.contains("<redacted>"));
    }
}
