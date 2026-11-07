//! What a run is given: the model endpoint and the run's own settings.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::hooks::Hooks;
use crate::permissions::settings::SandboxSettings;

/// The environment variable `glut` reads the model API's key from. Never handed on: not to a command (the shell's
/// cleaned environment drops it) and not to an MCP server's `${…}` header (`mcp::client`).
pub const API_KEY_ENV: &str = "GLUT_API_KEY";

/// The most model turns a run takes before it stops (a turn is one model call). Generous for real work, but an end
/// to a model that keeps calling tools without ever answering; 100 since T73b (40 ended a task of several files
/// mid-way). Budgets and the run's time limit are the real brakes.
pub const DEFAULT_MAX_TURNS: u32 = 100;
/// The most `maxTurns` in the settings and `/steps` may say.
pub const MOST_TURNS: u32 = 1000;

/// How long a whole run may take before it stops.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The most model turns one subagent (HS8, D6) takes of its own before it stops — its own budget, the parent's
/// `max_turns` still counting only the parent's calls.
pub const DEFAULT_SUBAGENT_MAX_TURNS: u32 = 15;

/// How long one subagent (HS8, D6) may run; at runtime it is additionally capped by the time the parent has
/// left, so a subagent can never outlive the run that started it. As long as a whole run: a model that thinks for
/// minutes first (one did for four, then had its finished work cut off at five) is working, not stuck — a stuck one
/// ends by the model's stall limit and each tool's own timeout, not by this.
pub const DEFAULT_SUBAGENT_TIMEOUT: Duration = DEFAULT_TIMEOUT;

/// How many subagents one run may start in all (HS8). Each brings up to [`DEFAULT_SUBAGENT_MAX_TURNS`] model
/// calls of its own, so without a count a parent asking for one per turn could multiply the run's spend by
/// that factor while staying inside every other limit.
pub const DEFAULT_MAX_SUBAGENTS: u32 = 8;

/// How many subagents of one reply run at once (TUI T40); the rest wait for one to end. More would meet a provider's
/// rate limit, and a local model serves one request at a time anyway.
pub const DEFAULT_PARALLEL_SUBAGENTS: u32 = 3;

/// The most `subagents.parallel` may say: as many as one run may start in all.
pub const MAX_PARALLEL_SUBAGENTS: u32 = DEFAULT_MAX_SUBAGENTS;

/// The size in KB from which a project's guide is pointed out as too large (TUI T42): twice the 4 KB `/init` aims at,
/// so a guide just over its aim is not nagged about.
pub const DEFAULT_GUIDE_WARN_KB: u32 = 8;

/// The most `guide.warnAt` may say: a guide larger than 64 KB is not read at all (`guide.rs`).
pub const MAX_GUIDE_WARN_KB: u32 = 64;

/// A model behind an OpenAI-compatible chat API (`POST {base_url}/v1/chat/completions`, or
/// `{base_url}/chat/completions` when [`Self::versioned`]): OpenRouter, Ollama and most others speak it.
#[derive(Clone)]
pub struct ModelEndpoint {
    /// The API's base, without `/v1` (`https://openrouter.ai/api`, `http://localhost:11434`) — or, when
    /// [`Self::versioned`], with its version (`https://openrouter.ai/api/v1`).
    pub base_url: String,
    /// Sent as a bearer token; `None` for an endpoint without one (a local Ollama).
    pub api_key: Option<String>,
    /// The model as the API names it (`deepseek/deepseek-chat`, `qwen3:8b`).
    pub model: String,
    /// How hard the model reasons before it answers; `None` sends nothing and leaves it to the provider.
    pub effort: Option<Effort>,
    /// `base_url` already ends in the API's version (`…/v1beta/openai`, `…/api/paas/v4`), as a provider of the
    /// catalog names it: requests go to `{base_url}/chat/completions`, not
    /// `{base_url}/v1/chat/completions`.
    pub versioned: bool,
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
            .field("versioned", &self.versioned)
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

/// What a model's tokens cost, in USD per million (`"prices"` in `~/.glut/settings.json`, or the caller's table): the
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

/// When a run has spent what it may (C6): checked before each model call, so the calls of
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
    /// Money spent that the usage at [`Self::prices`] does not show: subagents on models of their own (TUI T37a),
    /// counted at their prices — the difference to what the same tokens cost at the run's.
    pub paid_usd: f64,
}

impl Budget {
    /// What is left of the budget after `usage`: a subagent's, which counts from nothing, may spend only what its
    /// parent has not.
    pub fn less(&self, usage: &crate::model::Usage) -> Budget {
        let tokens = usage.budgeted_tokens();
        Budget {
            max_tokens: self.max_tokens.map(|max| max.saturating_sub(tokens)),
            max_usd: match (self.max_usd, self.cost_usd(usage)) {
                (Some(max), Some(cost)) => Some((max - cost).max(0.0)),
                (max, _) => max,
            },
            prices: self.prices,
            paid_usd: 0.0,
        }
    }

    /// What `usage` cost at the run's prices, with what was paid beside them; `None` without prices.
    pub fn cost_usd(&self, usage: &crate::model::Usage) -> Option<f64> {
        self.prices
            .map(|prices| prices.cost_usd(usage) + self.paid_usd)
    }

    /// Why `usage` is past the budget, `None` while it is not.
    pub fn spent(&self, usage: &crate::model::Usage) -> Option<String> {
        self.spent_costing(usage, self.cost_usd(usage))
    }

    /// Why `usage`, which cost `cost` (`None`: not known), is past the budget — for subagents side by side, whose
    /// money is added up at each one's own prices (TUI T37a).
    pub fn spent_costing(&self, usage: &crate::model::Usage, cost: Option<f64>) -> Option<String> {
        let tokens = usage.budgeted_tokens();
        if let Some(max) = self.max_tokens
            && tokens >= max
        {
            return Some(format!(
                "the run's budget of {max} tokens is spent ({tokens} used)"
            ));
        }
        if let (Some(max), Some(cost)) = (self.max_usd, cost)
            && cost >= max
        {
            return Some(format!(
                "the run's budget of ${max:.2} is spent (${cost:.2} used)"
            ));
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

/// The tools a host's screen adds for sessions side by side (TUI T36c): offered only where the run says so
/// ([`RunConfig::pane_tools`]) and never in a subagent — whatever the toolbox holds.
pub const PANE_TOOLS: [&str; 1] = ["tell"];

/// The tool a host's screen adds for a session that runs as a loop (TUI T60a): the model ends the loop with it once
/// the loop's goal is reached. Like [`PANE_TOOLS`] offered only where the run says so ([`RunConfig::loop_tool`]), never
/// in a subagent nor from a script.
pub const LOOP_TOOL: &str = "end_loop";

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
    /// More folders of skills beside the owner's `~/.glut/skills` and, in a trusted folder, the project's
    /// `.glut/skills` (HS6, K2): `<folder>/<name>/SKILL.md` each. `glut` fills them from its repeatable
    /// `--skill-dir`; a caller loads the catalog with them (`skills::Catalog::load`).
    pub skill_dirs: Vec<PathBuf>,
    /// The owner's hooks (HS7); empty: the run has none and nothing is built for them. Only the owner's
    /// `~/.glut/settings.json` contributes — a project's hooks are parsed and left out (`B1`).
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
    /// How many `task` calls of one reply run at once (TUI T40); 1 runs them one after another.
    pub parallel_subagents: u32,
    /// The named subagents a `task` call may name (TUI T37a): the owner's and the project's `agents/` folders.
    pub agents: std::sync::Arc<crate::agents::Catalog>,
    /// What builds the model of an agent that names its own; `None`: such an agent's calls fail.
    pub models: Option<crate::agents::Models>,
    /// Which model judges a command only the mode would ask about (T52); it needs [`Self::models`] to build one.
    pub classifier: crate::classifier::Choice,
    /// Whether the run offers `glut_file` from its start (TUI T37b): a turn that starts with a maker skill. Else it
    /// is offered from the moment the model loads one.
    pub glut_files: bool,
    /// Whether the run offers [`PANE_TOOLS`] (TUI T36c): the screen around it is split into panes of sessions that
    /// may write to each other. Never to a subagent.
    pub pane_tools: bool,
    /// Whether the run offers [`LOOP_TOOL`] (TUI T60a): the session runs as a loop of the owner's. Never to a subagent.
    pub loop_tool: bool,
    /// The images the run's prompt carries (T45): the owner's, pasted or `glut run --image`. Never a subagent's.
    pub prompt_images: Vec<crate::images::Image>,
    /// Where a tool's images are kept (T45: the owner's home, `~/.glut/images` below it); `None`: a tool's image is
    /// said in its answer and not shown.
    pub images_home: Option<PathBuf>,
    /// Whether the model sees images, as its provider's list says; `Some(false)`: a tool's image is not sent.
    pub sees_images: Option<bool>,
    /// Whether the run offers `codemode` and keeps its MCP tools behind it (Glut 2, A + E, G3/G8): the model then sees
    /// the core tools and `codemode`, and reaches the MCP tools from a script. On by default.
    pub codemode: bool,
    /// In code mode, whether the MCP tools are reached only from a script (G3). A run that declares its MCP tools — a
    /// skill — keeps them direct (G7, revised 2026-10-20 after measuring: a digest whose output *is* its data paid 3×
    /// the tokens through code); a chat turn keeps them behind code. On by default.
    pub mcp_behind_code: bool,
    /// Whether the run offers `todo_write`, its task list (Glut 2, D). On by default.
    pub todos: bool,
    /// The task list a turn before left (T73): the run starts with it and the model is shown it, so "go on" goes on
    /// with the same list. Empty: none, or all done.
    pub todo_list: Vec<crate::planning::TodoItem>,
    /// Whether the run asks its model once to go on when it ends with items open or with an announcement (T73).
    /// On by default; off for a loop's round and a plan's run, whose ends are their own.
    pub nudge: bool,
    /// Whether the run reminds its model of its task list while it works (T73, T80): a list left unchanged, or none
    /// written at all. The owner's `nudge` switch, but on in a loop's round and a plan's run, which work unwatched.
    pub remind: bool,
    /// The mode a run in Plan mode switches to once the owner approves its plan (`exit_plan`, Glut 2, D).
    pub build_mode: crate::permissions::Mode,
    /// Where the work folder's snapshots go before a run changes it (Glut 2, D, G11; `~/.glut/snapshots`), for `glut
    /// undo`. `None`: no snapshots.
    pub snapshots: Option<PathBuf>,
    /// Where the run journals its steps so it can be resumed after the process lost it (durable runs, D5). `None`:
    /// no journal. Only the run itself writes one, never a subagent.
    pub journal: Option<crate::journal::JournalSpec>,
    /// Where relocated results are kept beyond the run (Glut lean, N2) — a chat's conversation, whose next turn reads
    /// the ids its notes name. `None`: in memory only. Only the run itself, never a subagent (it shares the run's
    /// config but numbers its own texts).
    pub results: Option<crate::results::StoreSpec>,
    /// Where the run counts what Glut lean saves (N9; `~/.glut/lean.jsonl`, `glut gain`). `None`: only the `lean`
    /// events say it.
    pub savings: Option<PathBuf>,
    /// Whether the language servers serve the run (Glut 2, G12; T62): an edit's result carries the server's errors for
    /// the file, and the `lsp` tool is offered. On by default (the owner's `lsp`); without an installed server for a
    /// file's language nothing is added.
    pub diagnostics: bool,
    /// The language servers the run asks (T62). A host keeps one for a whole session and clones it into each turn's
    /// config — a new one starts the servers anew, and rust-analyzer indexes the project again; a run's subagents
    /// share it.
    pub servers: crate::diagnostics::LanguageServers,
    /// When refusals end the run (#112); the owner's `"refusals"` in `~/.glut/settings.json` sets them.
    pub refusals: RefusalLimits,
    /// When the run compacts itself (#111); the owner's `"compact"` and `"contextWindow"` set it.
    pub compaction: Compaction,
    /// The host goes on from a summary the run made ahead of its threshold and never put in (T63,
    /// `RunOutcome::prepared`): then a run that answered waits for it. Off, such a summary is dropped as the run ends.
    pub hand_on_prepared: bool,
    /// What the run may spend (C6); none by default.
    pub budget: Budget,
    /// What the owner sends while the run works (T10, steering): taken before the run's
    /// next model call, never between a reply and its results, and only by the run itself, never a subagent.
    pub inbox: Option<Inbox>,
    /// Secrets masked before they leave (T58): every tool result and the instructions, before the model, the events
    /// or a file of the run sees them. On by default — a host that started without the owner's
    /// `~/.glut/secrets.json` still masks with Glut's own formats; `None` masks nothing. Shared with the subagents.
    /// A host keeps one for a whole conversation (clone it into each turn's config): a new one numbers anew, and a
    /// placeholder of an earlier turn is then no longer written back.
    pub masks: Option<crate::masks::Masks>,
}

/// Messages for a run under way, from whoever started it (the TUI's Enter while Glut works). Shared: the sender keeps
/// one end, the run the other.
#[derive(Debug, Clone, Default)]
pub struct Inbox(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl Inbox {
    /// A message for the run's next step.
    pub fn send(&self, text: impl Into<String>) {
        self.lock().push(text.into());
    }

    /// Every message waiting, in the order sent; the inbox is empty after.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.lock())
    }

    /// The last message waiting, taken back before the run saw it.
    pub fn take_back(&self) -> Option<String> {
        self.lock().pop()
    }

    /// The messages waiting, as they stand.
    pub fn waiting(&self) -> Vec<String> {
        self.lock().clone()
    }

    /// Whether no message waits.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// A poisoned lock still holds the messages: a sender that panicked loses nothing of the others'.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
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
            parallel_subagents: DEFAULT_PARALLEL_SUBAGENTS,
            agents: std::sync::Arc::default(),
            models: None,
            classifier: crate::classifier::Choice::Session,
            glut_files: false,
            pane_tools: false,
            loop_tool: false,
            prompt_images: Vec::new(),
            images_home: None,
            sees_images: None,
            codemode: true,
            mcp_behind_code: true,
            todos: true,
            todo_list: Vec::new(),
            nudge: true,
            remind: true,
            build_mode: crate::permissions::Mode::Default,
            snapshots: None,
            journal: None,
            results: None,
            savings: None,
            diagnostics: true,
            servers: crate::diagnostics::LanguageServers::default(),
            refusals: RefusalLimits::DEFAULT,
            compaction: Compaction::default(),
            hand_on_prepared: false,
            budget: Budget::default(),
            masks: Some(crate::masks::Masks::default()),
            inbox: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inbox_gives_its_messages_in_order_and_takes_back_the_last() {
        let inbox = Inbox::default();
        let sender = inbox.clone();
        sender.send("a");
        sender.send("b");
        sender.send("c");
        assert_eq!(inbox.waiting(), ["a", "b", "c"]);
        assert_eq!(inbox.take_back().as_deref(), Some("c"));
        assert_eq!(inbox.take(), ["a", "b"]);
        assert!(inbox.is_empty() && sender.is_empty(), "one inbox, two ends");
    }

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
            versioned: false,
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
