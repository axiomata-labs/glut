//! What a run is given: the model endpoint and the run's own settings.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::hooks::Hooks;
use crate::permissions::settings::SandboxSettings;

/// The environment variable `axh` reads the model API's key from. Never handed on: not to a command (the shell's
/// cleaned environment drops it) and not to an MCP server's `${…}` header (`mcp::client`).
pub const API_KEY_ENV: &str = "AXH_API_KEY";

/// The most model turns a run takes before it stops (a turn is one model call). Generous for real work, but an end
/// to a model that keeps calling tools without ever answering.
pub const DEFAULT_MAX_TURNS: u32 = 40;

/// How long a whole run may take before it stops.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The most model turns one subagent (HS8, D6) takes of its own before it stops — its own budget, the parent's
/// `max_turns` still counting only the parent's calls.
pub const DEFAULT_SUBAGENT_MAX_TURNS: u32 = 15;

/// How long one subagent (HS8, D6) may run; at runtime it is additionally capped by the time the parent has
/// left, so a subagent can never outlive the run that started it.
pub const DEFAULT_SUBAGENT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How many subagents one run may start in all (HS8). Each brings up to [`DEFAULT_SUBAGENT_MAX_TURNS`] model
/// calls of its own, so without a count a parent asking for one per turn could multiply the run's spend by
/// that factor while staying inside every other limit.
pub const DEFAULT_MAX_SUBAGENTS: u32 = 8;

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
    /// The owner's hooks (HS7); empty: the run has none and nothing is built for them. Only the owner's
    /// `~/.axh/settings.json` contributes — a project's hooks are parsed and left out (`B1`).
    pub hooks: Hooks,
    /// How the hooks' commands run (HS3b's sandbox, HS7 D5); `None`: as they are, judged by the rules alone.
    pub sandbox: Option<SandboxSettings>,
    /// Whether the run offers the `task` tool (HS8, D7): `false` = no spec and no interception. The CLI keeps
    /// `true`; the core sets it from the skill's `allowed_tools` (HS8, B3).
    pub subagents: bool,
    /// The most model turns one subagent takes of its own (HS8, D6).
    pub subagent_max_turns: u32,
    /// How long one subagent may run, at runtime also capped by the parent's remaining time (HS8, D6).
    pub subagent_timeout: Duration,
    /// How many subagents the run may start in all; a `task` call past it is answered with an error.
    pub max_subagents: u32,
    /// Whether long tool results the model has answered on shrink in the conversation, their whole text kept for
    /// `read_full_result` (Glut 2, B). On by default: it needs nothing outside the run.
    pub shrink_results: bool,
    /// Whether the run offers `codemode` and keeps its MCP tools behind it (Glut 2, A + E, G3/G8): the model then sees
    /// the core tools and `codemode`, and reaches the MCP tools from a script. On by default.
    pub codemode: bool,
    /// In code mode, whether the MCP tools are reached only from a script (G3). A run that declares its MCP tools — a
    /// skill — keeps them direct (G7, revised 2026-10-20 after measuring: a digest whose output *is* its data paid 3×
    /// the tokens through code); a chat turn keeps them behind code. On by default.
    pub mcp_behind_code: bool,
    /// Whether the run offers `todo_write`, its task list (Glut 2, D). On by default.
    pub todos: bool,
    /// The mode a run in Plan mode switches to once the owner approves its plan (`exit_plan`, Glut 2, D).
    pub build_mode: crate::permissions::Mode,
    /// Where the work folder's snapshots go before a run changes it (Glut 2, D, G11; `~/.axh/snapshots`), for `axh
    /// undo`. `None`: no snapshots.
    pub snapshots: Option<PathBuf>,
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
            hooks: Hooks::default(),
            sandbox: None,
            subagents: true,
            subagent_max_turns: DEFAULT_SUBAGENT_MAX_TURNS,
            subagent_timeout: DEFAULT_SUBAGENT_TIMEOUT,
            max_subagents: DEFAULT_MAX_SUBAGENTS,
            shrink_results: true,
            codemode: true,
            mcp_behind_code: true,
            todos: true,
            build_mode: crate::permissions::Mode::Default,
            snapshots: None,
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
