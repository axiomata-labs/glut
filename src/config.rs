//! What a run is given: the model endpoint and the run's own settings.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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
    /// How hard the model reasons before it answers; `None` sends nothing and leaves it to the provider.
    pub effort: Option<Effort>,
}

/// How hard a reasoning model thinks before it answers (Glut effort, E2): the levels OpenRouter names. A model that
/// does not reason ignores it; a provider that knows fewer levels may refuse one it does not know, and says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// Every level, the least first.
    pub const ALL: [Effort; 7] = [
        Effort::None,
        Effort::Minimal,
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ];

    /// The level as the APIs, the settings and the flags spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::None => "none",
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
        }
    }

    /// The level `text` names, case and surrounding space aside.
    pub fn parse(text: &str) -> Option<Effort> {
        let text = text.trim();
        Effort::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(text))
    }

    /// [`Self::parse`], with an error that lists the levels there are.
    ///
    /// # Errors
    ///
    /// A message naming `text` and the known levels.
    pub fn parse_or_explain(text: &str) -> Result<Effort, String> {
        Effort::parse(text).ok_or_else(|| {
            let known: Vec<&str> = Effort::ALL.iter().map(|level| level.as_str()).collect();
            format!("unknown effort {text:?}; known: {}", known.join(", "))
        })
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The key never shows in a log line or an error, whatever prints the endpoint.
impl fmt::Debug for ModelEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelEndpoint")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("effort", &self.effort)
            .finish()
    }
}

/// How often a run may be refused before it ends (Glut robust, #112, R2–R4): a model that is refused keeps trying
/// the same call, or ways around it, until the step limit — each a paid turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefusalLimits {
    /// The same call (tool and target) refused this often ends the run; one time fewer, its answer warns.
    pub repeat: u32,
    /// Refusals of any kind in one run: from a third of it on each refused answer counts them, at it the run ends.
    pub total: u32,
}

impl RefusalLimits {
    /// The owner's go, 2026-10-21: the third identical refusal ends the run, and the fifteenth of any kind.
    pub const DEFAULT: RefusalLimits = RefusalLimits {
        repeat: 3,
        total: 15,
    };

    /// From which refusal of the run on each refused answer says how many there were.
    pub fn total_note_from(self) -> u32 {
        (self.total / 3).max(2)
    }
}

impl Default for RefusalLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The smallest context window taken from a flag, the settings, a catalog or a provider's list (#111): below it a
/// number is no window a run could work in, and compacting against it would summarise at every step.
pub const MIN_CONTEXT_WINDOW: u32 = 4_096;

/// What a model's tokens cost, in USD per million (`"prices"` in `~/.axh/settings.json`, or the caller's table): the
/// one thing a budget in money needs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    pub input_per_m: f64,
    /// Input read from the provider's prompt cache; unset, it costs `input_per_m` — never cheaper than written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_per_m: Option<f64>,
    pub output_per_m: f64,
}

impl Prices {
    /// What `usage` cost.
    pub fn cost_usd(&self, usage: &crate::model::Usage) -> f64 {
        let cached = usage.cached_input_tokens.min(usage.input_tokens);
        (usage.input_tokens - cached) as f64 / 1e6 * self.input_per_m
            + cached as f64 / 1e6 * self.cached_input_per_m.unwrap_or(self.input_per_m)
            + usage.output_tokens as f64 / 1e6 * self.output_per_m
    }

    /// Whether every price is a number a bill could hold: finite and not negative.
    pub fn valid(&self) -> bool {
        [
            self.input_per_m,
            self.cached_input_per_m.unwrap_or(0.0),
            self.output_per_m,
        ]
        .iter()
        .all(|price| price.is_finite() && *price >= 0.0)
    }
}

/// When a run has spent what it may (`docs/plans/glut-cards.md`, C6): checked before each model call, so the calls of
/// the last reply are all answered and the run ends as [`crate::FinishReason::Budget`]. Nothing set: no budget.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Budget {
    /// Tokens: input less what was read from the prompt cache, plus output — the measure a Studio session's limits
    /// use.
    pub max_tokens: Option<u64>,
    /// Money; counts only with [`Self::prices`].
    pub max_usd: Option<f64>,
    /// What the run's model costs.
    pub prices: Option<Prices>,
}

impl Budget {
    /// What is left of the budget after `usage`: a subagent's, which counts from nothing, may spend only what its
    /// parent has not.
    pub fn less(&self, usage: &crate::model::Usage) -> Budget {
        let tokens = usage
            .input_tokens
            .saturating_sub(usage.cached_input_tokens)
            .saturating_add(usage.output_tokens);
        Budget {
            max_tokens: self.max_tokens.map(|max| max.saturating_sub(tokens)),
            max_usd: match (self.max_usd, self.prices) {
                (Some(max), Some(prices)) => Some((max - prices.cost_usd(usage)).max(0.0)),
                (max, _) => max,
            },
            prices: self.prices,
        }
    }

    /// Why `usage` is past the budget, `None` while it is not.
    pub fn spent(&self, usage: &crate::model::Usage) -> Option<String> {
        let tokens = usage
            .input_tokens
            .saturating_sub(usage.cached_input_tokens)
            .saturating_add(usage.output_tokens);
        if let Some(max) = self.max_tokens
            && tokens >= max
        {
            return Some(format!(
                "the run's budget of {max} tokens is spent ({tokens} used)"
            ));
        }
        if let (Some(max), Some(prices)) = (self.max_usd, self.prices) {
            let cost = prices.cost_usd(usage);
            if cost >= max {
                return Some(format!(
                    "the run's budget of ${max:.2} is spent (${cost:.2} used)"
                ));
            }
        }
        None
    }
}

/// When a run compacts its conversation itself (Glut robust, #111, K1–K6): shortly before the model's context is
/// full, the older part becomes a summary by the run's own model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compaction {
    /// Compact near the limit and when the provider says the prompt is too long. On by default.
    pub auto: bool,
    /// The model's context in tokens, when the caller knows it (a flag, the settings, Axiomata's catalog); `None`:
    /// the run asks the provider's model list once the conversation is long enough to matter.
    pub context_window: Option<u32>,
}

impl Default for Compaction {
    fn default() -> Self {
        Compaction {
            auto: true,
            context_window: None,
        }
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
    /// Where the run journals its steps so it can be resumed after the process lost it (durable runs, D5). `None`:
    /// no journal. Only the run itself writes one, never a subagent.
    pub journal: Option<crate::journal::JournalSpec>,
    /// Where relocated results are kept beyond the run (Glut lean, N2) — a chat's conversation, whose next turn reads
    /// the ids its notes name. `None`: in memory only. Only the run itself, never a subagent (it shares the run's
    /// config but numbers its own texts).
    pub results: Option<crate::results::StoreSpec>,
    /// Where the run counts what Glut lean saves (N9; `~/.axh/lean.jsonl`, `axh gain`). `None`: only the `lean`
    /// events say it.
    pub savings: Option<PathBuf>,
    /// Whether an edit's result carries the language server's errors for the file (Glut 2, D, G12). On by default;
    /// without an installed server for the file's language nothing is added.
    pub diagnostics: bool,
    /// When refusals end the run (#112); the owner's `"refusals"` in `~/.axh/settings.json` sets them.
    pub refusals: RefusalLimits,
    /// When the run compacts itself (#111); the owner's `"compact"` and `"contextWindow"` set it.
    pub compaction: Compaction,
    /// What the run may spend (C6 of `docs/plans/glut-cards.md`); none by default.
    pub budget: Budget,
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
            codemode: true,
            mcp_behind_code: true,
            todos: true,
            build_mode: crate::permissions::Mode::Default,
            snapshots: None,
            journal: None,
            results: None,
            savings: None,
            diagnostics: true,
            refusals: RefusalLimits::DEFAULT,
            compaction: Compaction::default(),
            budget: Budget::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, cached: u64, output: u64) -> crate::model::Usage {
        crate::model::Usage {
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
        }
    }

    #[test]
    fn a_budget_counts_tokens_without_cache_reads_and_money_only_with_prices() {
        let none = Budget::default();
        assert_eq!(none.spent(&usage(u64::MAX / 2, 0, 1)), None);
        let tokens = Budget {
            max_tokens: Some(1_000),
            ..Budget::default()
        };
        assert_eq!(
            tokens.spent(&usage(1_500, 600, 99)),
            None,
            "900 + 99 is under 1000"
        );
        assert!(
            tokens
                .spent(&usage(1_500, 600, 100))
                .unwrap()
                .contains("1000 tokens")
        );
        let prices = Prices {
            input_per_m: 1.0,
            cached_input_per_m: Some(0.1),
            output_per_m: 2.0,
        };
        assert!((prices.cost_usd(&usage(2_000_000, 1_000_000, 1_000_000)) - 3.1).abs() < 1e-9);
        let money = Budget {
            max_usd: Some(3.0),
            prices: Some(prices),
            ..Budget::default()
        };
        assert!(
            money
                .spent(&usage(2_000_000, 1_000_000, 1_000_000))
                .is_some()
        );
        let unpriced = Budget {
            max_usd: Some(0.0),
            ..Budget::default()
        };
        assert_eq!(
            unpriced.spent(&usage(1, 0, 1)),
            None,
            "money without prices is no budget"
        );
        assert!(
            !Prices {
                input_per_m: f64::NAN,
                ..prices
            }
            .valid()
        );
        assert!(prices.valid());
        assert_eq!(money.less(&usage(1_000_000, 0, 0)).max_usd, Some(2.0));
        assert_eq!(tokens.less(&usage(700, 200, 100)).max_tokens, Some(400));
    }

    #[test]
    fn the_key_is_never_printed() {
        let endpoint = ModelEndpoint {
            base_url: "https://example.test/api".into(),
            api_key: Some("sk-secret-123".into()),
            model: "m".into(),
            effort: None,
        };
        let printed = format!("{endpoint:?}");
        assert!(!printed.contains("sk-secret-123"), "{printed}");
        assert!(printed.contains("<redacted>"));
    }

    #[test]
    fn an_effort_reads_in_any_case_and_an_unknown_one_lists_the_levels() {
        assert_eq!(Effort::parse(" High "), Some(Effort::High));
        assert_eq!(Effort::parse("xhigh"), Some(Effort::Xhigh));
        for level in Effort::ALL {
            assert_eq!(Effort::parse(level.as_str()), Some(level));
            assert_eq!(
                serde_json::to_value(level).unwrap(),
                serde_json::json!(level.as_str())
            );
        }
        let err = Effort::parse_or_explain("extreme").unwrap_err();
        assert!(
            err.contains("\"extreme\"") && err.contains("none, minimal"),
            "{err}"
        );
    }
}
