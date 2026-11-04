//! Who may do what in a run (`docs/plans/harness.md`, HS3a, P1–P8): rules in Claude Code's format, modes, and asking
//! the owner.
//!
//! Every tool says what a call would touch ([`Access`]); the [`Gate`] judges it against the rules — `deny` beats
//! `ask` beats `allow`, across every source — and, where no rule speaks, against the run's [`Mode`]. An `ask` goes to
//! the run's [`Asker`]: the terminal, the app, or nobody (refused). Rules are patterns, not a fence: the sandbox of
//! HS3b is what stops a command that slips past them.

pub mod command;
pub mod kinds;
pub mod settings;

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::{Component, Path};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;

use command::{Analysis, analyze, names_protected};

use crate::folders::Folders;
use crate::keychain::Keychain;

/// What a tool call would touch, as the tool itself says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Reading below the working folder (`read_file`, `list_dir`, `grep`, `glob`); the path as the model gave it.
    Read { path: String },
    /// Changing a file below the working folder (`write_file`, `edit_file`).
    Edit { path: String },
    /// A shell command, and the folder the shell is in (relative to the working folder; empty for the folder
    /// itself) — where a relative path the command writes lands.
    Bash {
        command: String,
        folder: String,
        /// The model asked to run it outside the sandbox (HS3b): never allowed by a rule nor by `auto`, always asked.
        unsandboxed: bool,
        /// It runs inside the sandbox — there is one and the model did not ask to leave it. Only then does what the
        /// command *is* count ([`kinds`]): a reader free in every mode, `auto` asking only the heavy ones.
        sandboxed: bool,
    },
    /// A sandboxed command reaching a host through the run's proxy (HS3b): the host name, lowercased.
    Network { host: String },
    /// An MCP tool call (HS5): the server's name and the tool's own name on it. The model sees it as
    /// `mcp__<server>__<tool>`, which is what the rules are written as.
    Mcp { server: String, tool: String },
    /// Only the run's own state (`bash_output`, `kill_shell`): always allowed.
    Own,
    /// A change of the project's memory (TUI T35): what it would keep or remove, and where, as the owner reads it. It
    /// outlives the run and is read into every later session — so it is asked even in `auto`, never done in plan mode.
    Memory { change: String },
    /// A write of Glut's own files — a skill, an agent, the owner's hooks (TUI T37b): what and where, as the owner
    /// reads it. Like [`Self::Memory`] it is asked even in `auto` and refused in plan mode, and an answer never
    /// reaches beyond this one write: a hook is a command that runs on every call from then on.
    GlutFile { change: String },
    /// A `task` call that names an agent (TUI T37a): which, and on what it runs where its file says, as the owner
    /// reads it in an ask. Judged as [`Self::Other`] is — the rules know `task` by its name.
    Agent { what: String },
    /// A message from this pane's model to another pane of the screen (TUI T36c, `tell`): to whom and what, as the
    /// owner reads it in an ask. Known to the rules by the tool's name; asked in plan mode too — a planner writes to
    /// the pane that builds, which changes nothing here.
    Pane { what: String },
    /// A tool the rules know only by its name.
    Other,
}

/// What the rules' tool names stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    Read,
    Edit,
    Bash,
    /// `Network(crates.io)`, `Network(*.github.com)`: a host the run's proxy lets a sandboxed command reach — and
    /// `web_fetch` too, which asks the same question.
    Network,
    /// `WebFetch`, `WebFetch(domain:docs.rs)` — Claude Code's spelling, so one `allowed_tools` line means the same on
    /// both harnesses: a host `web_fetch` may reach, and nothing else. Kept apart from [`Self::Network`] so a skill
    /// that may read pages does not also let its shell (`curl`, `pip`) reach every host (glut-round CP2, audit).
    Web,
    /// `mcp__mail` (every tool of that server) or `mcp__mail__send` (one tool) (HS5, M5): a name the model sees
    /// for an MCP tool, with no pattern — there is nothing a pattern could add to it.
    Mcp,
    /// A tool named directly (`grep`, a future MCP tool).
    Tool,
}

/// The names Claude Code's format gives meaning to; anything spelled like one but not exactly is a typo (P4).
const KNOWN_KINDS: [&str; 7] = [
    "Read",
    "Edit",
    "Write",
    "Bash",
    "Network",
    "WebFetch",
    "WebSearch",
];

/// How a rule names an MCP tool (HS5, M5): `mcp__<server>` for a whole server, `mcp__<server>__<tool>` for one.
const MCP_PREFIX: &str = "mcp__";

/// Path globs ignore case where the file system does: on macOS `src/A.rs` and `src/a.rs` are one file.
const CASE_INSENSITIVE_PATHS: bool = cfg!(target_os = "macos");

/// One rule: `Bash`, `Bash(cargo test:*)`, `Read(src/**)`, `Edit(**/*.rs)`, or a tool's own name.
#[derive(Clone)]
pub struct Rule {
    text: String,
    kind: RuleKind,
    /// The tool's own name for a [`RuleKind::Tool`] rule.
    tool: String,
    pattern: Option<Pattern>,
    /// An allow rule from a project's `.glut/` (T74b): it lets a command run, never with the Keychain open.
    from_project: bool,
}

#[derive(Clone)]
enum Pattern {
    /// `prefix:*`: the command's first words are these, followed by anything or nothing.
    Prefix(Vec<String>),
    /// A command pattern with `*` for any run of characters, held against the words joined by single spaces.
    Wild(String),
    /// A path glob below the working folder; `*` stays within one folder, `**` crosses them.
    Path(GlobMatcher),
    /// A host: exactly this name, or with a leading `*.` any name below it (not the name itself).
    Host(String),
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Rule {
    /// Reads a rule.
    ///
    /// # Errors
    ///
    /// A text that is no rule: empty, an unclosed `(`, an empty name, a name spelled like `Bash`/`Read`/`Edit`/`Write`
    /// but not exactly, or a path pattern that is no glob.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (name, pattern) = match text.split_once('(') {
            Some((name, rest)) => {
                let pattern = rest
                    .strip_suffix(')')
                    .ok_or_else(|| format!("rule {text:?} has no closing parenthesis"))?;
                (name, Some(pattern.trim()))
            }
            None => (text, None),
        };
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("rule {text:?} names no tool"));
        }
        // `bash(rm:*)` would otherwise be a rule about a tool called "bash(…)" that never matches: a deny that
        // silently does nothing.
        if let Some(known) = KNOWN_KINDS
            .iter()
            .find(|known| known.eq_ignore_ascii_case(name) && **known != name)
        {
            return Err(format!("rule {text:?}: write {known}, not {name}"));
        }
        let kind = match name {
            "Read" => RuleKind::Read,
            "Edit" | "Write" => RuleKind::Edit,
            "Bash" => RuleKind::Bash,
            "Network" => RuleKind::Network,
            "WebFetch" => RuleKind::Web,
            // M5: an MCP rule is written the way the model is shown the tool, and says everything in its name.
            // `mcp__mail(send)` would be a rule about a tool called "mail(send)" that never matches — a deny that
            // silently does nothing — so a pattern here is refused rather than kept.
            _ if name.starts_with(MCP_PREFIX) => {
                if pattern.is_some() {
                    return Err(format!("rule {text:?}: an MCP rule takes no pattern"));
                }
                if name == MCP_PREFIX {
                    return Err(format!("rule {text:?} names no server"));
                }
                RuleKind::Mcp
            }
            _ => RuleKind::Tool,
        };
        let pattern = match pattern.filter(|p| !p.is_empty() && *p != "*" && *p != "**") {
            None => None,
            Some(p) if matches!(kind, RuleKind::Network | RuleKind::Web) => Some(Pattern::Host(
                host_pattern(p.strip_prefix("domain:").unwrap_or(p))?,
            )),
            Some(p) if kind == RuleKind::Bash => Some(match p.strip_suffix(":*") {
                // `Bash(:*)` names no command; read as a prefix of nothing it would match nothing sensibly.
                Some(prefix) if rule_words(prefix).is_empty() => {
                    return Err(format!("rule {text:?} names no command before `:*`"));
                }
                Some(prefix) => Pattern::Prefix(rule_words(prefix)),
                None => Pattern::Wild(rule_words(p).join(" ")),
            }),
            Some(p) => Some(Pattern::Path(
                GlobBuilder::new(&normal_path(p))
                    .literal_separator(true)
                    .case_insensitive(CASE_INSENSITIVE_PATHS)
                    .build()
                    .map_err(|err| format!("rule {text:?}: {err}"))?
                    .compile_matcher(),
            )),
        };
        Ok(Rule {
            text: text.to_owned(),
            kind,
            // Claude Code's `WebSearch` names Glut's `web_search` (glut-round CP3): one `allowed_tools` line, both
            // harnesses.
            tool: if name == "WebSearch" {
                crate::tools::search::NAME.to_owned()
            } else {
                name.to_owned()
            },
            pattern,
            from_project: false,
        })
    }

    /// Whether the rule may give a command the Keychain (T74b): the owner's own, and — for a command — naming at
    /// least a program and its sub-command (`gh pr *`, `git push *`, never `gh *`, `git *` or `Bash`): a wide
    /// "always" from a card that did not ask for the Keychain never opens it later.
    fn gives_keychain(&self) -> bool {
        if self.from_project {
            return false;
        }
        if self.kind != RuleKind::Bash {
            return true;
        }
        let fixed = match &self.pattern {
            Some(Pattern::Prefix(words)) => words.len(),
            Some(Pattern::Wild(text)) => {
                let head = text.split('*').next().unwrap_or_default();
                let words = head.split_whitespace().count();
                // `gh p*`: the last word is cut by the star, not fixed.
                if text.contains('*') && !head.ends_with(' ') {
                    words.saturating_sub(1)
                } else {
                    words
                }
            }
            _ => 0,
        };
        fixed >= 2
    }

    /// The rule as a project's settings give it (T74b): it allows what it names, but never gives the Keychain — only
    /// the owner's own rules do.
    pub fn from_project(mut self) -> Self {
        self.from_project = true;
        self
    }

    /// The rule as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether this rule names `tool` as a tool call of its own (HS8, D10): a plain tool rule without a
    /// pattern — exactly the call [`restrict_allows`] would pass for that name. The core reads a skill's
    /// `allowed_tools` with it to decide whether the run offers `task` at all (HS8, B3).
    pub fn names_tool(&self, tool: &str) -> bool {
        self.kind == RuleKind::Tool && self.tool == tool && self.pattern.is_none()
    }

    /// Whether this rule could name some call of `tool`, whose calls touch what `sample` stands for — the rule's
    /// pattern left aside (glut-round CP4): `Edit(News/**)` could name a call of `edit_file`, `Read` none. What the
    /// toolbox offers the model is decided on this, so a run is not shown tools its declaration refuses outright.
    fn could_name(&self, tool: &str, sample: &Access) -> bool {
        let named = self.kind == RuleKind::Tool && self.tool == tool;
        match sample {
            Access::Read { .. } => self.kind == RuleKind::Read || named,
            Access::Edit { .. } => self.kind == RuleKind::Edit || named,
            Access::Bash { .. } => self.kind == RuleKind::Bash || named,
            Access::Network { .. } => {
                self.kind == RuleKind::Network
                    || (self.kind == RuleKind::Web && tool == crate::tools::web::NAME)
            }
            Access::Mcp {
                server,
                tool: called,
            } => self.kind == RuleKind::Mcp && mcp_matches(&self.tool, server, called),
            Access::Own => true,
            Access::Other
            | Access::Memory { .. }
            | Access::Agent { .. }
            | Access::Pane { .. }
            | Access::GlutFile { .. } => named,
        }
    }

    /// Whether this rule is about commands of `tool`.
    fn is_command_rule(&self, tool: &str) -> bool {
        self.kind == RuleKind::Bash || (self.kind == RuleKind::Tool && self.tool == tool)
    }

    /// Whether the rule speaks about one simple command, given as its words.
    fn matches_words(&self, words: &[String]) -> bool {
        match &self.pattern {
            None => true,
            // The command word compares as the file system finds commands (`RM` is `rm` on macOS); the rest exactly —
            // `git` and most tools tell `push` from `PUSH`.
            Some(Pattern::Prefix(prefix)) => {
                words.len() >= prefix.len()
                    && prefix
                        .first()
                        .zip(words.first())
                        .is_none_or(|(want, have)| command::same_command(want, have))
                    && words[1..prefix.len()] == prefix[1..]
            }
            Some(Pattern::Wild(pattern)) => {
                let fold = |text: &str| match text.split_once(' ') {
                    Some((first, rest)) if command::CASE_INSENSITIVE_COMMANDS => {
                        format!("{} {rest}", first.to_lowercase())
                    }
                    _ if command::CASE_INSENSITIVE_COMMANDS => text.to_lowercase(),
                    _ => text.to_owned(),
                };
                let (pattern, text) = (fold(pattern), fold(&words.join(" ")));
                // `git push *` also means a bare `git push`, as in Claude Code.
                wildcard(&pattern, &text)
                    || pattern.strip_suffix(" *").is_some_and(|bare| bare == text)
            }
            Some(Pattern::Path(_) | Pattern::Host(_)) => false,
        }
    }

    /// Whether the rule speaks about reading or editing `path` with `tool`.
    fn matches_path(&self, tool: &str, access: &Access) -> bool {
        let (path, kind) = match access {
            Access::Read { path } => (path, RuleKind::Read),
            Access::Edit { path } => (path, RuleKind::Edit),
            Access::Network { host } => {
                let speaks = match self.kind {
                    RuleKind::Network => true,
                    RuleKind::Web => tool == crate::tools::web::NAME,
                    _ => false,
                };
                return speaks
                    && match &self.pattern {
                        None => true,
                        Some(Pattern::Host(pattern)) => host_matches(pattern, host),
                        Some(_) => false,
                    };
            }
            // M5: `mcp__mail` speaks about every tool of `mail`, `mcp__mail__send` about that one tool. The tool
            // the model called is in the rule's name already, so `tool` (the model-facing name) is not asked.
            Access::Mcp {
                server,
                tool: called,
            } => {
                return self.kind == RuleKind::Mcp && mcp_matches(&self.tool, server, called);
            }
            _ => return self.kind == RuleKind::Tool && self.tool == tool && self.pattern.is_none(),
        };
        if self.kind != kind && !(self.kind == RuleKind::Tool && self.tool == tool) {
            return false;
        }
        match &self.pattern {
            None => true,
            // `Read(secrets/**)` speaks about the folder `secrets` too, not only what is in it.
            Some(Pattern::Path(glob)) => {
                let path = normal_path(path);
                glob.is_match(&path) || glob.is_match(format!("{path}/"))
            }
            Some(_) => false,
        }
    }
}

/// A host pattern as written in `Network(…)`: a name, or `*.` and a name; letters, digits, dots and dashes only.
fn host_pattern(text: &str) -> Result<String, String> {
    let lower = text.trim().to_ascii_lowercase();
    let name = lower.strip_prefix("*.").unwrap_or(&lower);
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(format!(
            "Network({text}): a host is a name like crates.io or *.github.com"
        ));
    }
    Ok(lower)
}

/// Whether `host` (lowercased) is what `pattern` names.
fn host_matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.') && rest.len() > 1),
        None => host == pattern,
    }
}

/// Whether an MCP rule names this call (M5): `mcp__<server>` every tool of the server, `mcp__<server>__<tool>`
/// exactly one. The rule text is what the model is shown, so the comparison runs against the server's own names —
/// a server called `mail` with a tool `send` is `mcp__mail__send`, and a server whose name itself holds `__` is
/// still read correctly (the whole rest after `mcp__` is the server, or the server up to the next `__`).
fn mcp_matches(rule: &str, server: &str, tool: &str) -> bool {
    let Some(rest) = rule.strip_prefix(MCP_PREFIX) else {
        return false;
    };
    rest == server
        || rest
            .strip_prefix(&format!("{server}__"))
            .is_some_and(|called| called == tool)
}

/// A rule's command, read by the same lexer as the commands it is held against, so `Bash(git  status:*)` and
/// `git status` agree.
fn rule_words(text: &str) -> Vec<String> {
    let analysis = analyze(text);
    match analysis.parts.into_iter().next() {
        Some(part) if analysis.opaque.is_empty() => {
            part.assignments.into_iter().chain(part.words).collect()
        }
        _ => text.split_whitespace().map(str::to_owned).collect(),
    }
}

/// A path as the rules see it: relative, `.` and `..` resolved by their names, doubled `/` gone, `.` for the folder
/// itself. A path that climbs out of the folder keeps its leading `..` — no rule below the folder matches it, and the
/// file tools refuse it.
pub fn normal_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut above = 0usize;
    for component in Path::new(path.trim()).components() {
        match component {
            Component::Normal(name) => parts.push(name.to_str().unwrap_or("")),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    above += 1;
                }
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    let mut out: Vec<&str> = std::iter::repeat_n("..", above).collect();
    out.extend(parts);
    if out.is_empty() {
        ".".into()
    } else {
        out.join("/")
    }
}

/// Whether `text` matches `pattern`, where `*` stands for any run of characters (none included).
fn wildcard(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest)) = parts.split_first() else {
        return text.is_empty();
    };
    let Some(mut tail) = text.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return tail.is_empty();
    };
    for part in middle {
        match tail.find(part) {
            Some(at) => tail = &tail[at + part.len()..],
            None => return false,
        }
    }
    tail.ends_with(last)
}

/// The run's default for what no rule speaks about (P3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Reads free — a command that only reads too ([`kinds::reads_only`]) —; edits and other commands asked.
    #[default]
    Default,
    /// Reads and edits free; commands asked. The screen calls it `build`.
    AcceptEdits,
    /// Reads only: edits refused even where a rule allows them, commands refused unless a rule allows them, nothing
    /// asked.
    Plan,
    /// Whatever no rule allows is refused; nothing is asked.
    DontAsk,
    /// Whatever no rule denies or asks about is allowed (the owner's auto mode) — except a heavy command
    /// ([`kinds::heavy`]: `rm -rf`, `git push`, `sudo`, …) and one asking to leave the sandbox, which are asked.
    Auto,
}

impl Mode {
    /// The mode as the flags and settings spell it (`plan`, `accept_edits`, …).
    pub fn name(self) -> &'static str {
        match self {
            Mode::Default => "default",
            Mode::AcceptEdits => "accept_edits",
            Mode::Plan => "plan",
            Mode::DontAsk => "dont_ask",
            Mode::Auto => "auto",
        }
    }

    /// Reads a mode as the flags and settings write it (`accept_edits`, also `acceptEdits` as Claude Code does).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "default" => Some(Mode::Default),
            "accept_edits" | "acceptEdits" | "build" => Some(Mode::AcceptEdits),
            "plan" => Some(Mode::Plan),
            "dont_ask" | "dontAsk" => Some(Mode::DontAsk),
            "auto" => Some(Mode::Auto),
            _ => None,
        }
    }

    /// How much a mode lets through without a rule, least first — a project's settings may only pick a mode at least
    /// as strict as the owner's own (P5).
    pub fn looseness(self) -> u8 {
        match self {
            Mode::Plan => 0,
            Mode::DontAsk => 1,
            Mode::Default => 2,
            Mode::AcceptEdits => 3,
            Mode::Auto => 4,
        }
    }

    /// Whether the mode never asks: an ask becomes a refusal.
    fn asks_nothing(self) -> bool {
        matches!(self, Mode::Plan | Mode::DontAsk)
    }

    fn fallback(self, access: &Access) -> Verdict {
        use Verdict::{Allow, Ask, Deny};
        match (self, access) {
            (Mode::Auto, Access::Memory { .. } | Access::GlutFile { .. }) => Ask,
            // `auto` trusts the sandbox to hold a command: where there is none, every command asks (security model
            // review, 2026-11-04) — but what a rule of the owner's allows, which never reaches the fallback.
            (
                Mode::Auto,
                Access::Bash {
                    sandboxed: false, ..
                },
            ) => Ask,
            (Mode::Auto, _) => Allow,
            (_, Access::Read { .. }) => Allow,
            (Mode::Plan | Mode::Default | Mode::AcceptEdits, Access::Pane { .. }) => Ask,
            (Mode::AcceptEdits, Access::Edit { .. }) => Allow,
            (Mode::Plan | Mode::DontAsk, _) => Deny,
            (Mode::Default | Mode::AcceptEdits, _) => Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Allow,
    Ask,
    Deny,
}

/// The rules of a run, from every source together.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    pub allow: Vec<Rule>,
    pub ask: Vec<Rule>,
    pub deny: Vec<Rule>,
}

/// What the rules say about a call: a verdict and why, or nothing (the mode decides), or that the call cannot be
/// read well enough for a rule or the `auto` mode to allow it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Said {
    Verdict(Verdict, String),
    /// Asked whatever the mode, refused where the mode asks nothing.
    Unsure(String),
    Silent,
}

impl RuleSet {
    /// These rules with only the allow rules that may give a command the Keychain (T74b, [`Rule::gives_keychain`]).
    fn owners(&self) -> RuleSet {
        RuleSet {
            allow: self
                .allow
                .iter()
                .filter(|rule| rule.gives_keychain())
                .cloned()
                .collect(),
            ask: self.ask.clone(),
            deny: self.deny.clone(),
        }
    }

    /// Adds `other`'s rules to these.
    pub fn extend(&mut self, other: RuleSet) {
        self.allow.extend(other.allow);
        self.ask.extend(other.ask);
        self.deny.extend(other.deny);
    }

    /// What the rules say about `access` of `tool`.
    #[cfg(test)]
    fn verdict(&self, tool: &str, access: &Access) -> Said {
        self.verdict_with(tool, access, true, &Folders::default())
    }

    /// [`Self::verdict`]; `readers_free`: a part of a command that only reads needs no rule of its own — not for an
    /// `allowed-tools` narrowing, which names every part it lets through.
    ///
    /// `beside`: the folders beside the working one (T60c), where a command writes and reads as inside it.
    fn verdict_with(
        &self,
        tool: &str,
        access: &Access,
        readers_free: bool,
        beside: &Folders,
    ) -> Said {
        match access {
            Access::Bash {
                command,
                folder,
                unsandboxed,
                sandboxed,
            } => {
                let said = self.command_verdict(
                    tool,
                    &analyze(command),
                    folder,
                    readers_free && *sandboxed,
                    beside,
                );
                match said {
                    // Outside the sandbox the operating system holds nothing back: only a deny stays a deny, everything
                    // else is the owner's to answer.
                    Said::Verdict(Verdict::Deny, _) => said,
                    _ if *unsandboxed => {
                        Said::Unsure("the command asks to run outside the sandbox".into())
                    }
                    _ => said,
                }
            }
            Access::Own => Said::Verdict(Verdict::Allow, "the run's own state".into()),
            other => {
                let first = |rules: &[Rule]| {
                    rules
                        .iter()
                        .find(|rule| rule.matches_path(tool, other))
                        .map(|rule| rule.text.clone())
                };
                if let Some(rule) = first(&self.deny) {
                    return Said::Verdict(Verdict::Deny, format!("rule {rule}"));
                }
                if let Some(rule) = first(&self.ask) {
                    return Said::Verdict(Verdict::Ask, format!("rule {rule}"));
                }
                match first(&self.allow) {
                    Some(rule) => Said::Verdict(Verdict::Allow, format!("rule {rule}")),
                    None => Said::Silent,
                }
            }
        }
    }

    /// A command's verdict (P2): deny and ask rules against every part and what its wrappers run; what it writes held
    /// against the edit rules; allowed by rules only when every bit of it was read and every part is allowed.
    fn command_verdict(
        &self,
        tool: &str,
        analysis: &Analysis,
        folder: &str,
        readers_free: bool,
        beside: &Folders,
    ) -> Said {
        let candidates = analysis.deny_candidates();
        let matching = |rules: &[Rule]| {
            rules
                .iter()
                .find(|rule| {
                    rule.is_command_rule(tool)
                        && candidates.iter().any(|words| rule.matches_words(words))
                })
                .map(|rule| rule.text.clone())
        };
        if let Some(rule) = matching(&self.deny) {
            return Said::Verdict(Verdict::Deny, format!("rule {rule}"));
        }
        let mut written_inside = Vec::new();
        for target in &analysis.writes {
            if names_protected(target) {
                return Said::Verdict(
                    Verdict::Deny,
                    format!("the command writes the protected path {target}"),
                );
            }
            // A relative path lands where the shell is, not at the top of the working folder; a full one may name a
            // folder beside it (T60c), judged as the file tools' paths there are.
            let in_beside = beside.rule_path(target).filter(|_| !target.contains('$'));
            let inside_beside = in_beside.is_some();
            let path = in_beside.unwrap_or_else(|| normal_path(&format!("{folder}/{target}")));
            if !inside_beside
                && (target.starts_with(['/', '~'])
                    || target.contains('$')
                    || path.starts_with(".."))
            {
                return Said::Unsure(format!(
                    "the command writes {target}, outside the working folder"
                ));
            }
            let edit = Access::Edit { path: path.clone() };
            if let Some(rule) = self
                .deny
                .iter()
                .find(|rule| rule.matches_path("write_file", &edit))
            {
                return Said::Verdict(
                    Verdict::Deny,
                    format!("rule {} (the command writes {path})", rule.text),
                );
            }
            if let Some(rule) = self
                .ask
                .iter()
                .find(|rule| rule.matches_path("write_file", &edit))
            {
                return Said::Verdict(
                    Verdict::Ask,
                    format!("rule {} (the command writes {path})", rule.text),
                );
            }
            written_inside.push(edit);
        }
        if let Some(rule) = matching(&self.ask) {
            return Said::Verdict(Verdict::Ask, format!("rule {rule}"));
        }
        // A `cd` in the same command moves where its relative writes land, past what was just checked.
        let moves = analysis.parts.iter().any(|part| {
            part.words.first().is_some_and(|word| {
                ["cd", "pushd", "popd"]
                    .iter()
                    .any(|c| command::same_command(c, word))
            })
        });
        if moves && !analysis.writes.is_empty() {
            return Said::Unsure("the command changes folder and writes a file".into());
        }
        if analysis.is_opaque() {
            return Said::Unsure(format!(
                "the rules cannot read it: {}",
                analysis.opaque.join(", ")
            ));
        }
        if analysis.may_name_protected() {
            return Said::Unsure("the command names a protected path".into());
        }
        // Allowed by rules: no part with assignments in front (`DYLD_INSERT_LIBRARIES=… cargo test` is another
        // program), every part allowed, every file it writes allowed by an edit rule.
        if analysis.parts.is_empty()
            || analysis
                .parts
                .iter()
                .any(|part| !part.assignments.is_empty())
        {
            return Said::Silent;
        }
        let mut by = Vec::new();
        for part in &analysis.parts {
            match self
                .allow
                .iter()
                .find(|rule| rule.is_command_rule(tool) && rule.matches_words(&part.words))
            {
                Some(rule) => by.push(rule.text.clone()),
                // A part that only reads needs no rule of its own (`cargo test | head`), as it needs none alone.
                None if readers_free
                    && kinds::reader(&part.words)
                    && self
                        .reads_beyond(&part.words, &analysis.reads, folder, beside)
                        .is_none() => {}
                None => return Said::Silent,
            }
        }
        if by.is_empty() {
            return Said::Silent;
        }
        for edit in &written_inside {
            match self
                .allow
                .iter()
                .find(|rule| rule.matches_path("write_file", edit))
            {
                Some(rule) => by.push(rule.text.clone()),
                None => return Said::Silent,
            }
        }
        by.dedup();
        Said::Verdict(Verdict::Allow, format!("rule {}", by.join(", ")))
    }

    /// Why a reading command's words (its program first) or the files it redirects in are not free to read, if they
    /// are not: a path out of the working folder (absolute, `~`, `$`, `..`) — `read_file` reaches no further —, or a
    /// file a rule refuses or asks about; where such a rule exists, a word the rule cannot be matched against
    /// (`git show HEAD:path`) counts as one. An option's value (`--file=.env`) is a word too.
    fn reads_beyond(
        &self,
        words: &[String],
        redirected: &[String],
        folder: &str,
        beside: &Folders,
    ) -> Option<&'static str> {
        let any_read_rule = self
            .deny
            .iter()
            .chain(&self.ask)
            .any(|rule| rule.kind == RuleKind::Read);
        let paths = words.iter().skip(1).chain(redirected).filter_map(|word| {
            match word.strip_prefix('-') {
                Some(option) => option.split_once('=').map(|(_, value)| value),
                None => Some(word.as_str()),
            }
        });
        for word in paths {
            // A folder beside the working one (T60c) reads as the working folder does, its rules included.
            if let Some(path) = beside.rule_path(word).filter(|_| !word.contains('$')) {
                if any_read_rule && self.read_held_back(&path) {
                    return Some("the command reads a file a rule holds back");
                }
                continue;
            }
            if word.starts_with(['/', '~', '$'])
                || normal_path(&format!("{folder}/{word}")).starts_with("..")
            {
                return Some(READS_OUTSIDE);
            }
            if any_read_rule
                && (word.contains(':')
                    || self.read_held_back(&normal_path(&format!("{folder}/{word}"))))
            {
                return Some("the command reads a file a rule holds back");
            }
        }
        None
    }

    /// Whether reading `path` is refused or asked by a rule — for a search, which leaves such files out of its
    /// results rather than asking about each one.
    pub fn read_held_back(&self, path: &str) -> bool {
        let access = Access::Read {
            path: path.to_owned(),
        };
        self.deny
            .iter()
            .chain(&self.ask)
            .any(|rule| rule.matches_path("read_file", &access))
    }
}

tokio::task_local! {
    /// The subagent whose calls are judged while it runs beside others (TUI T40): its `task` call's id and its label.
    /// Task-local, so it follows the subagent's future through every await, also when the parent polls several at
    /// once in one task; a judgement made on a task of its own (the sandbox's proxy) counts as the run's.
    static SUBAGENT: (String, String);
    /// What a named agent may call (TUI T37a): its `tools`, narrowing the gate for its own calls only, on top of
    /// every narrowing of the run — task-local for the same reason as [`SUBAGENT`].
    static AGENT_NARROWING: Option<Arc<Narrowing>>;
}

/// Runs `future` as the subagent of the `task` call `id`, labelled `label`: its judgements are its own
/// ([`Gate::take_log`]) and its asks name it ([`AskRequest::from`]).
pub async fn as_subagent<F: Future>(id: String, label: String, future: F) -> F::Output {
    as_agent(id, label, None, future).await
}

/// [`as_subagent`] for a named agent (TUI T37a): `tools` — `(by, rules)`, its name for a refusal and its rules —
/// narrow what its calls may do; an empty list allows no call at all (an agent's `tools: []`), `None` narrows
/// nothing.
pub async fn as_agent<F: Future>(
    id: String,
    label: String,
    tools: Option<(String, Vec<Rule>)>,
    future: F,
) -> F::Output {
    let narrowing = tools.map(|(by, rules)| Arc::new(Narrowing { by, rules }));
    SUBAGENT
        .scope((id, label), AGENT_NARROWING.scope(narrowing, future))
        .await
}

/// The subagent the current future runs as, if any.
fn subagent() -> Option<(String, String)> {
    SUBAGENT.try_with(Clone::clone).ok()
}

/// Whether a sandboxed command of the current future may have the network (T37a): always, but for a named agent whose
/// `tools` name no `Network` rule — its commands then run with no way out, since the proxy that judges connections
/// does so on a task of its own and cannot see the agent's narrowing.
pub fn agent_reaches_network() -> bool {
    agent_narrowing().is_none_or(|narrowing| {
        narrowing
            .rules
            .iter()
            .any(|rule| rule.kind == RuleKind::Network)
    })
}

/// The narrowing of the named agent the current future runs as, if any.
fn agent_narrowing() -> Option<Arc<Narrowing>> {
    AGENT_NARROWING.try_with(Clone::clone).ok().flatten()
}

/// The rules', narrowings' and mode's word on one access before anyone is asked ([`Gate::weigh`]).
struct Weighed {
    verdict: Verdict,
    reason: String,
    /// The program that would get the Keychain (T74), if any.
    keychain: Option<String>,
    /// Only the mode or the rules' unsureness asks about this sandboxed command — no rule of the owner's, no heavy
    /// kind, no Keychain: a second model may say it only reads (T52, [`Judge`]).
    judgeable: bool,
}

/// A question to the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskRequest {
    pub tool: String,
    /// What the call would do, on one line: the command, the path.
    pub target: String,
    /// How far an allowance from then on may reach, narrowest first, as the owner reads it (`git status --short`,
    /// `git status *`, `git *`): [`Answer::AllowWithin`] picks one. Empty: none can be given (a command the rules
    /// cannot read), only this once.
    pub scopes: Vec<String>,
    /// The subagent that asks, by its task as the parent labelled it (TUI T40); `None`: the run itself.
    pub from: Option<String>,
    /// The command would run with the Keychain open (T74): a listed program, the Keychain shut to every other.
    pub keychain: bool,
    /// What the second model said of the command when it did not find it only reading (T52), to show beside it.
    pub note: Option<String>,
}

/// The owner's own version of an edit asked about (T91): the whole text they let be written, and what the model
/// proposed, for the note that tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnersText {
    pub text: String,
    pub proposed: String,
    /// The file the diff showed the change of; `None`: there was none. The owner's text is written only over it.
    pub based_on: Option<String>,
}

/// What an open question shows, set for as long as it is open: cleared when it ends, however it ends.
struct Shown<'a>(&'a Mutex<Option<serde_json::Value>>);

impl<'a> Shown<'a> {
    fn set(slot: &'a Mutex<Option<serde_json::Value>>, value: Option<serde_json::Value>) -> Self {
        *slot.lock().unwrap_or_else(|poison| poison.into_inner()) = value;
        Shown(slot)
    }
}

impl Drop for Shown<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) = None;
    }
}

/// The owner's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allow,
    /// Allow this and the same again from then on — for the rest of the run, in Glut's TUI the whole session: the
    /// narrowest of the request's scopes (for a command exactly this command).
    AllowAlways,
    /// Allow this and from then on what the request's scope at this index reaches (`git status *`, `git *`).
    AllowWithin(u8),
    Deny,
}

/// What an [`Asker`] returns.
pub type AnswerFuture<'a> = Pin<Box<dyn Future<Output = Answer> + Send + 'a>>;

/// Who answers an ask: the terminal, the app, or nobody.
pub trait Asker: Send + Sync {
    fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a>;

    /// Whether someone is there to answer; `false` refuses every ask without asking, and says so.
    fn present(&self) -> bool {
        true
    }

    /// Shows a server's question to the owner and returns their answer (T67) — a form to fill in, a page to open, or
    /// a form Glut declined for asking a secret, which is only told. An asker that shows no forms declines them.
    fn fill<'a>(
        &'a self,
        _asked: &'a crate::mcp::elicit::Asked,
    ) -> crate::mcp::elicit::ReplyFuture<'a> {
        Box::pin(async { crate::mcp::elicit::Reply::Decline })
    }
}

/// A second model's word on a command the gate would otherwise ask about (T52): whether it only reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opinion {
    /// It reads, lists, searches or prints and nothing else: it runs like a listed reader, in the sandbox.
    pub reads_only: bool,
    /// Why, in a few words.
    pub why: String,
    /// The model that judged, as the owner reads it.
    pub by: String,
}

/// A [`Judge`]'s answer; `None` when it gave no clear one — the gate then asks as without it.
pub type OpinionFuture<'a> = Pin<Box<dyn Future<Output = Option<Opinion>> + Send + 'a>>;

/// What judges a command the gate cannot read as only reading (T52): it sees the command and its folder alone —
/// never the conversation, a file or an output, so nothing a page or a file says reaches it. It can only make a
/// command run as a reader would; a write, a heavy command, an owner's rule and the sandbox stay as they are.
pub trait Judge: Send + Sync {
    /// The model judging, as the owner reads it; a judge of another name forgets what the last one said.
    fn name(&self) -> String;
    fn judge<'a>(&'a self, command: &'a str, folder: &'a str) -> OpinionFuture<'a>;
    /// What its calls used since last asked: the run counts it as its own, budget and all.
    fn take_usage(&self) -> crate::model::Usage {
        crate::model::Usage::default()
    }
}

/// The most opinions a gate keeps; past it they start afresh.
const MOST_OPINIONS: usize = 512;
/// How long a judge that gave no answer is passed by: a judge that is down must not make every new command wait.
const JUDGE_RESTS: std::time::Duration = std::time::Duration::from_secs(60);

/// Nobody to ask (headless): every ask is refused.
pub struct Nobody;

impl Asker for Nobody {
    fn ask<'a>(&'a self, _request: &'a AskRequest) -> AnswerFuture<'a> {
        Box::pin(async { Answer::Deny })
    }

    fn present(&self) -> bool {
        false
    }
}

/// Asks in the terminal: the question on stderr, the answer a line on stdin — `y`/`j` for once, the whole word
/// `always`/`immer` from then on (a single letter next to `y` would be one slip away), anything else no.
pub struct Terminal;

/// How much of a command an ask shows; the rest is counted, so a long harmless start cannot push the real action off
/// the screen unseen.
const ASK_DISPLAY_CHARS: usize = 300;

impl Asker for Terminal {
    fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a> {
        let target = ask_display(&request.target);
        let prompt = format!(
            "  ? {} {target} — allow? [y]es / always / [n]o: ",
            printable(&request.tool)
        );
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                use std::io::{BufRead, Write};
                let mut err = std::io::stderr().lock();
                let _ = write!(err, "{prompt}");
                let _ = err.flush();
                let mut line = String::new();
                if std::io::stdin().lock().read_line(&mut line).is_err() {
                    return Answer::Deny;
                }
                match line.trim().to_lowercase().as_str() {
                    "y" | "j" | "yes" | "ja" => Answer::Allow,
                    "always" | "immer" => Answer::AllowAlways,
                    _ => Answer::Deny,
                }
            })
            .await
            .unwrap_or(Answer::Deny)
        })
    }
}

/// A command for an ask: line breaks shown as `⏎` (each one starts a command of its own), invisible characters
/// removed, and a long one shortened in the middle — its start and its end both stay in sight, so a long harmless
/// beginning cannot hide what comes last.
pub fn ask_display(text: &str) -> String {
    let marked: String = text.replace('\n', " ⏎ ");
    let shown = printable(&marked);
    let count = shown.chars().count();
    if count <= ASK_DISPLAY_CHARS {
        return shown;
    }
    let half = ASK_DISPLAY_CHARS / 2;
    let head: String = shown.chars().take(half).collect();
    let tail: String = shown.chars().skip(count - half).collect();
    format!(
        "{head} … ({} characters left out) … {tail}",
        count - 2 * half
    )
}

/// `text` as plain characters: no control characters, and none of the invisible ones that reorder or hide text
/// (bidirectional overrides, zero-width characters, line and paragraph separators) — a model's command must not
/// write escape sequences into the owner's terminal, nor show something other than what runs.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || is_invisible(c) {
                ' '
            } else {
                c
            }
        })
        .collect()
}

pub(crate) fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{034F}'
        | '\u{061C}'
        | '\u{180E}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{E0100}'..='\u{E01EF}'
        | '\u{115F}'
        | '\u{1160}'
        | '\u{17B4}'
        | '\u{17B5}'
        | '\u{2800}'
        | '\u{3164}'
        | '\u{FFA0}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{2028}'
        | '\u{2029}'
        | '\u{FEFF}'
        | '\u{E0000}'..='\u{E007F}')
}

/// How a call was judged, for the run's events (P8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    pub tool: String,
    pub target: String,
    pub allowed: bool,
    /// Why: the rule, the mode, or the owner's answer.
    pub reason: String,
}

/// An allowance an "always" answer gave: the scope as the ask named it (`git status *`) and the rules it became.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Granted {
    pub label: String,
    pub rules: Vec<String>,
}

/// The hosts an arena's model reaches (T61): the package registries of the languages its tasks are in, each only
/// serving what it holds — not Go's module proxy, which fetches from any host a module path names (security audit:
/// a way out for anything a model reads).
pub const ARENA_HOSTS: [&str; 7] = [
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    "registry.npmjs.org",
    "pypi.org",
    "files.pythonhosted.org",
    "registry.yarnpkg.com",
];

/// Whether `host` is one of [`ARENA_HOSTS`], on the web's own ports or none named.
fn arena_host(host: &str) -> bool {
    let name = match host.rsplit_once(':') {
        Some((name, "443" | "80")) => name,
        Some(_) => return false,
        None => host,
    };
    ARENA_HOSTS.contains(&name.to_ascii_lowercase().as_str())
}

/// Judges every tool call of a run.
pub struct Gate {
    rules: Mutex<RuleSet>,
    /// The run's mode; switched once when the owner approves a plan (`set_mode`).
    mode: Mutex<Mode>,
    asker: Arc<dyn Asker>,
    /// One question at a time: the proxy's connections can ask while a command runs, and two prompts in one terminal
    /// would mix their answers.
    asking: tokio::sync::Mutex<()>,
    /// The whole arguments of the call the open question is about (T91), while it is open: an edit's diff in the
    /// editor needs its text, which the call's event carries cut. Never an event's, a journal's or the model's.
    asked_arguments: Mutex<Option<serde_json::Value>>,
    /// What a rename's open question shows (T91): each file as it would leave it, set only by
    /// [`Gate::judge_all_showing`] — never from a call's arguments, which the model writes.
    asked_rename: Mutex<Option<serde_json::Value>>,
    /// The owner's own text for the open question's edit (T91), given beside the answer ([`Gate::answer_with_text`]).
    owners_text: Mutex<Option<OwnersText>>,
    /// The owner's text of an edit allowed, by the call it was given for, until the toolbox takes it
    /// ([`Gate::take_owners_text`]).
    owners_answer: Mutex<Option<(serde_json::Value, OwnersText)>>,
    /// Each judgement under the subagent's `task` call it was made for ([`as_subagent`]); `None`: the run's own.
    log: Mutex<Vec<(Option<String>, Judgement)>>,
    /// What the owner's "always" answers allowed from then on, oldest first: the scope as the owner read it and the
    /// rules it became — shown and taken back through [`Gate::granted`] and [`Gate::take_back`] (TUI T15).
    granted: Mutex<Vec<(String, Vec<Rule>)>>,
    /// The `allowed-tools` lists that narrow this run (HS4 C3, HS6 K5, HS6b B3) — a command's, a skill's, or both.
    /// Every one of them must name a call, so a second narrowing can only take away from the first, never give
    /// back what it left out. Added through [`Gate::narrow`].
    narrowings: Vec<Narrowing>,
    /// A pane's role (TUI T36c): its agent's `tools`, narrowing every run of the session — its subagents' too, whose
    /// own narrowing comes on top — and changed while the session runs ([`Gate::set_role`]).
    role: Mutex<Option<Arc<Narrowing>>>,
    /// The folders beside the working one (T60c), shared with the run's tools ([`crate::Toolbox::set_gate`]).
    beside: Mutex<Folders>,
    /// The tool whose next command the gate judges as run where it stands (the shell), held weakly: it holds the gate.
    /// What a command's glob finds there decides whether it names a protected file ([`Analysis::protected_in`]).
    here: Mutex<Option<std::sync::Weak<dyn crate::tools::Tool>>>,
    /// The Keychain's list and switch (T74), shared with the run's shell ([`crate::Toolbox::set_gate`]); a gate
    /// without one keeps it shut and asks for every listed program.
    keychain: Mutex<Keychain>,
    /// The commands this gate let run with the Keychain and the listed program each was judged for (T74b): the shell
    /// opens it for what was judged, not for what the list says a moment later.
    keychain_given: Mutex<Vec<(String, String)>>,
    /// The second model that judges commands only the mode would ask about (T52); `None`: none, the owner is asked.
    judge: Mutex<Option<Arc<dyn Judge>>>,
    /// What it said, by folder and command: the same command again costs nothing.
    opinions: Mutex<HashMap<(String, String), Opinion>>,
    /// When the judge last gave no answer ([`JUDGE_RESTS`]).
    judge_failed: Mutex<Option<std::time::Instant>>,
    /// An arena's model (T61, [`Gate::set_contained`]): nothing is ever asked — what would be is refused, so no model
    /// waits for the owner while another works on.
    contained: std::sync::atomic::AtomicBool,
    /// Its commands write their own folder alone ([`crate::sandbox::Confinement`]): a heavy one that reaches no further
    /// ([`kinds::reaches_out`]) runs.
    confined: std::sync::atomic::AtomicBool,
}

/// One `allowed-tools` list and who declared it: only the calls these rules name are judged as usual, everything
/// else is refused — and such a rule never allows anything by itself.
#[derive(Debug, Clone)]
struct Narrowing {
    /// Who narrowed the run, as the refusal names it (`the command /review`, `the skill mail-digest`).
    by: String,
    rules: Vec<Rule>,
}

impl Gate {
    pub fn new(rules: RuleSet, mode: Mode, asker: Arc<dyn Asker>) -> Self {
        Gate {
            rules: Mutex::new(rules),
            mode: Mutex::new(mode),
            asker,
            asking: tokio::sync::Mutex::new(()),
            asked_arguments: Mutex::new(None),
            asked_rename: Mutex::new(None),
            owners_text: Mutex::new(None),
            owners_answer: Mutex::new(None),
            log: Mutex::new(Vec::new()),
            granted: Mutex::new(Vec::new()),
            narrowings: Vec::new(),
            role: Mutex::new(None),
            beside: Mutex::new(Folders::default()),
            here: Mutex::new(None),
            keychain: Mutex::new(Keychain::default()),
            keychain_given: Mutex::new(Vec::new()),
            judge: Mutex::new(None),
            opinions: Mutex::new(HashMap::new()),
            judge_failed: Mutex::new(None),
            contained: std::sync::atomic::AtomicBool::new(false),
            confined: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The second model that judges what only the mode would ask about (T52), or none. Another judge than the last
    /// forgets what that one said.
    pub fn set_judge(&self, judge: Option<Arc<dyn Judge>>) {
        let mut held = self
            .judge
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if held.as_ref().map(|judge| judge.name()) != judge.as_ref().map(|judge| judge.name()) {
            self.opinions
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .clear();
        }
        *held = judge;
    }

    /// Whether a rule of any file asks about or refuses a read (`Read(…)`): then no judge decides a command's reads.
    fn reads_ruled(&self) -> bool {
        let rules = self
            .rules
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        rules
            .ask
            .iter()
            .chain(&rules.deny)
            .any(|rule| rule.kind == RuleKind::Read)
    }

    /// The judge's word on `command` in `folder`: kept from before, or asked now.
    async fn opinion(&self, command: &str, folder: &str) -> Option<Opinion> {
        let key = (folder.to_owned(), command.to_owned());
        if let Some(said) = self
            .opinions
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&key)
        {
            return Some(said.clone());
        }
        let judge = self
            .judge
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()?;
        let resting = |failed: &Option<std::time::Instant>| {
            failed.is_some_and(|at| at.elapsed() < JUDGE_RESTS)
        };
        if resting(
            &self
                .judge_failed
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        ) {
            return None;
        }
        let Some(said) = judge.judge(command, folder).await else {
            *self
                .judge_failed
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(std::time::Instant::now());
            return None;
        };
        // Kept only while it is still the judge: another one set meanwhile forgot the last one's words.
        let current = self
            .judge
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .is_some_and(|now| now.name() == judge.name());
        if current {
            let mut opinions = self
                .opinions
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if opinions.len() >= MOST_OPINIONS {
                opinions.clear();
            }
            opinions.insert(key, said.clone());
        }
        Some(said)
    }

    /// What the judge's calls used since this was last asked (T52): the run counts it as its own.
    pub fn take_judge_usage(&self) -> crate::model::Usage {
        self.judge
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .map(|judge| judge.take_usage())
            .unwrap_or_default()
    }

    /// Asks before a listed program gets the Keychain, by `keychain`'s list and switch (T74).
    pub fn set_keychain(&self, keychain: Keychain) {
        *self
            .keychain
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = keychain;
    }

    /// The listed program [`Self::judge`] let `command` run with the Keychain for, taken once: the shell opens it
    /// for exactly what was judged (T74, T74b) — a list or a switch changed between the two widens nothing.
    pub fn keychain_given(&self, command: &str) -> Option<String> {
        let mut given = self
            .keychain_given
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let at = given.iter().position(|(judged, _)| judged == command)?;
        Some(given.remove(at).1)
    }

    /// The listed program a sandboxed command would run with the Keychain, when it is shut: then the command is
    /// asked in every mode, whatever it is, unless a rule of the owner's allows it.
    fn wants_keychain(&self, access: &Access) -> Option<String> {
        let Access::Bash {
            command,
            sandboxed: true,
            ..
        } = access
        else {
            return None;
        };
        self.keychain
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .wants(command)
    }

    /// Judges commands against `folders` too, the folders beside the working one (T60c).
    pub fn set_beside(&self, folders: Folders) {
        *self
            .beside
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = folders;
    }

    /// Ties the gate to the tool whose folder a command runs in (the shell, [`crate::Toolbox::set_gate`]).
    pub(crate) fn set_here(&self, tool: std::sync::Weak<dyn crate::tools::Tool>) {
        *self
            .here
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(tool);
    }

    /// Whether `analysis` names a protected path where the shell's next command runs; without a shell tied to the
    /// gate, a glob that might expand to one counts.
    fn names_protected_here(&self, analysis: &Analysis) -> bool {
        let here = self
            .here
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .and_then(|tool| tool.here());
        match here {
            Some(folder) => analysis.protected_in(&folder),
            None => analysis.may_name_protected(),
        }
    }

    /// The folders beside the working one.
    fn beside(&self) -> Folders {
        self.beside
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// The role the session's pane plays now (TUI T36c): `by` names it in a refusal, `rules` are what it may call —
    /// an empty list is no tool at all. `None`: no role, nothing narrowed beyond the run's own.
    pub fn set_role(&self, role: Option<(String, Vec<Rule>)>) {
        *self
            .role
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) =
            role.map(|(by, rules)| Arc::new(Narrowing { by, rules }));
    }

    /// The rules of the role the session's pane plays, as written; `None` without a role (an empty list: no tool).
    pub fn role_rules(&self) -> Option<Vec<String>> {
        self.role_narrowing()
            .map(|role| role.rules.iter().map(|rule| rule.text.clone()).collect())
    }

    /// Whether a sandboxed command may have the network under the session's role (T36c): as for a named subagent,
    /// only a role whose rules name a `Network` host has it.
    pub fn role_reaches_network(&self) -> bool {
        self.role_narrowing()
            .is_none_or(|role| role.rules.iter().any(|rule| rule.kind == RuleKind::Network))
    }

    /// The role's narrowing, if the session has one.
    fn role_narrowing(&self) -> Option<Arc<Narrowing>> {
        self.role
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// What the owner's "always" answers allowed from then on, oldest first.
    pub fn granted(&self) -> Vec<Granted> {
        self.granted
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .map(|(label, rules)| Granted {
                label: label.clone(),
                rules: rules.iter().map(|rule| rule.text.clone()).collect(),
            })
            .collect()
    }

    /// Takes back the allowance an "always" answer gave as `rules` ([`Granted::rules`]: a label alone can stand for
    /// two, reading and editing one path): they leave the allow list — the last copy of each, the one the answer
    /// added, so the same rule in a settings file stays. `false` when no answer gave these rules.
    pub fn take_back(&self, rules: &[String]) -> bool {
        let taken = {
            let mut granted = self
                .granted
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let Some(at) = granted.iter().rposition(|(_, given)| {
                given.len() == rules.len()
                    && given
                        .iter()
                        .zip(rules)
                        .all(|(rule, text)| rule.text == *text)
            }) else {
                return false;
            };
            granted.remove(at).1
        };
        let mut rules = self
            .rules
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for rule in taken {
            if let Some(at) = rules.allow.iter().rposition(|held| held.text == rule.text) {
                rules.allow.remove(at);
            }
        }
        true
    }

    /// Narrows the run to the calls `rules` name (HS4 C3, HS6 K5), on top of every narrowing it already has: a
    /// call must be named by each of them. `by` is how a refusal names the declarer (`the command /review`). An
    /// empty list declares nothing and narrows nothing.
    pub fn narrow(&mut self, by: impl Into<String>, rules: Vec<Rule>) {
        if !rules.is_empty() {
            self.narrowings.push(Narrowing {
                by: by.into(),
                rules,
            });
        }
    }

    /// Whether the run offers the model `tool`, whose calls touch what `sample` stands for (glut-round CP4): not when
    /// a narrowing names no call of it, a deny rule without a pattern refuses all of them, or — for an edit tool —
    /// the mode is Plan, which writes nothing whatever a rule says. Every tool's schema rides along on every turn, so
    /// a tool that can only be refused costs tokens and invites a call that fails. Offering is not allowing: what is
    /// offered is still judged call by call.
    pub fn offers(&self, tool: &str, sample: &Access) -> bool {
        if *sample == Access::Own {
            return true;
        }
        if self.mode() == Mode::Plan && matches!(sample, Access::Edit { .. }) {
            return false;
        }
        let denied = self
            .rules
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .deny
            .iter()
            .any(|rule| rule.pattern.is_none() && rule.could_name(tool, sample));
        let agent = agent_narrowing();
        let role = self.role_narrowing();
        !denied
            && self
                .narrowings
                .iter()
                .chain(agent.as_deref())
                .chain(role.as_deref())
                .all(|narrowing| {
                    narrowing
                        .rules
                        .iter()
                        .any(|rule| rule.could_name(tool, sample))
                })
    }

    /// Whether every narrowing of the run names this one command by a pattern of its own (`Bash(axiomata-cli board
    /// add:*)`, not a bare `Bash`): the declaration the shell asks before it starts one of its direct programs
    /// outside the sandbox ([`crate::tools::shell::DirectProgram`]). A run nothing narrowed declares no command.
    pub fn declares_command(&self, words: &[String]) -> bool {
        // An agent's `tools` never declare one (T37a): a project's agent file is no source of a command run outside
        // the sandbox; `judge` still applies its narrowing to the call.
        !self.narrowings.is_empty()
            && self.narrowings.iter().all(|narrowing| {
                narrowing.rules.iter().any(|rule| {
                    rule.kind == RuleKind::Bash
                        && rule.pattern.is_some()
                        && rule.matches_words(words)
                })
            })
    }

    /// Who narrowed this run, in the order they did — empty for a run nothing narrowed.
    pub fn narrowed_by(&self) -> Vec<&str> {
        self.narrowings
            .iter()
            .map(|narrowing| narrowing.by.as_str())
            .collect()
    }

    pub fn mode(&self) -> Mode {
        *self
            .mode
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Switches the run's mode — out of Plan once the owner approved the plan (Glut 2, `exit_plan`).
    pub fn set_mode(&self, mode: Mode) {
        *self
            .mode
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = mode;
    }

    /// Whether anyone can be asked at all (a terminal, the app) — `false` for a headless run and an arena's model.
    pub fn can_ask(&self) -> bool {
        self.present()
    }

    /// Makes this gate an arena model's (T61): every question is refused at once, with that reason, and the network
    /// reaches the package registries alone ([`ARENA_HOSTS`]). `confined`: the sandbox holds its commands to their own
    /// folder ([`crate::sandbox::Confinement`], set through [`crate::Toolbox::contain`]), so a heavy command that
    /// reaches no further runs — never judged by its words alone, which cannot tell where a write lands (a link made
    /// earlier, an option, a moved `HEAD`). Never taken back.
    pub fn set_contained(&self, confined: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        self.contained.store(true, Relaxed);
        if confined {
            self.confined.store(true, Relaxed);
        }
    }

    fn is_confined(&self) -> bool {
        self.confined.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn is_contained(&self) -> bool {
        self.contained.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Someone to ask: an asker that is there, and no arena's model.
    fn present(&self) -> bool {
        !self.is_contained() && self.asker.present()
    }

    /// Why an ask was refused unasked.
    fn nobody(&self) -> &'static str {
        if self.is_contained() {
            "an arena's model asks no one"
        } else {
            "no one to ask"
        }
    }

    /// Asks the owner about something that is no single call (Glut 2: leaving Plan mode with a plan): one question at
    /// a time like every ask, refused without asking when nobody is there.
    pub async fn ask_owner(&self, tool: &str, target: &str) -> Answer {
        self.ask_owner_within(tool, target, Vec::new()).await
    }

    /// [`Self::ask_owner`] with choices beyond yes and no: [`Answer::AllowWithin`] picks one of `scopes` (`exit_plan`
    /// offers the modes to leave Plan into).
    pub async fn ask_owner_within(&self, tool: &str, target: &str, scopes: Vec<String>) -> Answer {
        self.ask_owner_showing(tool, target, scopes, None).await
    }

    /// What a rename's open question shows, while it is open (T91).
    pub fn asked_rename(&self) -> Option<serde_json::Value> {
        self.asked_rename
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// [`Self::ask_owner_within`] with a rename's files to show beside its target ([`Self::asked_rename`]).
    async fn ask_owner_showing(
        &self,
        tool: &str,
        target: &str,
        scopes: Vec<String>,
        rename: Option<&serde_json::Value>,
    ) -> Answer {
        if !self.present() {
            return Answer::Deny;
        }
        let request = AskRequest {
            tool: tool.to_owned(),
            target: target.to_owned(),
            scopes,
            from: subagent().map(|(_, label)| label),
            keychain: false,
            note: None,
        };
        let _one_at_a_time = self.asking.lock().await;
        let _shown = Shown::set(&self.asked_rename, rename.cloned());
        self.asker.ask(&request).await
    }

    /// Puts a server's question to the owner (T67): one question at a time like every ask, declined without asking
    /// when nobody is there, cancelled when the owner has not answered within [`crate::mcp::elicit::WAIT`]. Only the
    /// owner answers it: the asker shows it, the model never sees it.
    pub async fn fill(&self, mut asked: crate::mcp::elicit::Asked) -> crate::mcp::elicit::Reply {
        use crate::mcp::elicit::{Reply, WAIT};
        if !self.present() {
            return Reply::Decline;
        }
        asked.from = subagent().map(|(_, label)| label);
        let _one_at_a_time = self.asking.lock().await;
        tokio::time::timeout(WAIT, self.asker.fill(&asked))
            .await
            .unwrap_or(Reply::Cancel)
    }

    /// The mode as the flags and settings spell it (`plan`, `accept_edits`, …).
    pub fn mode_name(&self) -> &'static str {
        self.mode().name()
    }

    /// The rules this run is judged by, as written — for `/permissions` (HS4).
    pub fn rules(&self) -> RuleSet {
        self.rules
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Whether a rule refuses or asks about reading `path`: a search leaves such files out of what it shows.
    pub fn read_held_back(&self, path: &str) -> bool {
        self.rules
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .read_held_back(path)
    }

    /// Judges `access` of `tool`: `Ok` to run it, `Err` with the reason to refuse it. Asks the owner where the rules
    /// or the mode say so.
    pub async fn judge(&self, tool: &str, access: &Access) -> Result<(), String> {
        self.judge_call(tool, access, None).await
    }

    /// The arguments of the call the open question is about, while one is open (T91): the screen shows an edit's
    /// diff from them.
    pub fn asked_arguments(&self) -> Option<serde_json::Value> {
        self.asked_arguments
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Gives the owner's own text for the edit the open question is about, before it is answered (T91: changed on a
    /// diff's right side in the editor). Taken only with an allowing answer, and only while a question is open.
    pub fn answer_with_text(&self, owners: OwnersText) {
        if self.asked_arguments().is_some() && self.asked_rename().is_none() {
            *self
                .owners_text
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(owners);
        }
    }

    /// The owner's text for the allowed call with `arguments`, once: the toolbox writes it instead of the model's.
    pub fn take_owners_text(&self, arguments: &serde_json::Value) -> Option<OwnersText> {
        let mut answer = self
            .owners_answer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if answer.as_ref().is_some_and(|(asked, _)| asked == arguments) {
            answer.take().map(|(_, owners)| owners)
        } else {
            None
        }
    }

    /// [`Self::judge`] for a call whose whole `arguments` a question may show ([`Self::asked_arguments`]).
    pub async fn judge_call(
        &self,
        tool: &str,
        access: &Access,
        arguments: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let target = describe(access);
        if *access == Access::Own {
            return Ok(());
        }
        // T74b: a Keychain given to this command before and not taken goes; this judgement decides afresh.
        if let Access::Bash { command, .. } = access {
            self.keychain_given
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .retain(|(judged, _)| judged != command);
        }
        let Weighed {
            mut verdict,
            mut reason,
            keychain,
            judgeable,
        } = self.weigh(tool, access, &target)?;
        // T61: an arena's model reaches the package registries alone — a fair comparison fetches no page, and its code
        // sends nothing elsewhere.
        if self.is_contained()
            && let Access::Network { host } = access
            && !arena_host(host)
        {
            verdict = Verdict::Deny;
            reason = format!("an arena's model reaches only the package registries, not {host}");
        }
        // T52: what only the mode would ask about, a second model may find only reading — then it runs as a listed
        // reader does, in the sandbox; else its word goes beside the question.
        let mut note = None;
        if verdict == Verdict::Ask
            && judgeable
            && let Access::Bash {
                command, folder, ..
            } = access
            && let Some(said) = self.opinion(command, folder).await
        {
            if said.reads_only {
                verdict = Verdict::Allow;
                reason = format!("{JUDGED_READING} {}: {}", said.by, said.why);
            } else {
                note = Some(format!("{}: {}", said.by, said.why));
            }
        }
        let allowed = match verdict {
            Verdict::Allow => true,
            Verdict::Deny => false,
            Verdict::Ask if !self.present() => {
                reason = format!("{} ({reason})", self.nobody());
                false
            }
            Verdict::Ask => {
                let scopes = scopes(tool, access, keychain.is_some());
                let request = AskRequest {
                    tool: tool.to_owned(),
                    // T74b: a 🔑 card says where the command runs when it is not the working folder's top — git finds
                    // its repository, its config and its hooks there.
                    target: match access {
                        Access::Bash { folder, .. } if keychain.is_some() && !folder.is_empty() => {
                            format!("{target}   ⟨{folder}/⟩")
                        }
                        _ => target.clone(),
                    },
                    scopes: scopes.iter().map(|scope| scope.label.clone()).collect(),
                    from: subagent().map(|(_, label)| label),
                    keychain: keychain.is_some(),
                    note,
                };
                let _one_at_a_time = self.asking.lock().await;
                let take = || {
                    self.owners_text
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .take()
                };
                take();
                let answer = {
                    // Cleared when the question ends, answered or dropped with its run.
                    let _shown = Shown::set(&self.asked_arguments, arguments.cloned());
                    self.asker.ask(&request).await
                };
                let owners = take();
                if let (Some(owners), Some(arguments)) = (owners, arguments)
                    && matches!(
                        answer,
                        Answer::Allow | Answer::AllowAlways | Answer::AllowWithin(_)
                    )
                    && matches!(access, Access::Edit { .. })
                {
                    *self
                        .owners_answer
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner()) =
                        Some((arguments.clone(), owners));
                }
                let within = match answer {
                    Answer::AllowAlways => Some(0),
                    Answer::AllowWithin(at) => Some(usize::from(at)),
                    _ => None,
                };
                match answer {
                    Answer::Allow => {
                        reason = "allowed when asked".into();
                        true
                    }
                    Answer::AllowAlways | Answer::AllowWithin(_) => {
                        // An index past the last scope (an answer typed elsewhere) takes the narrowest, never more.
                        let scope = within.and_then(|at| scopes.get(at)).or(scopes.first());
                        reason = match scope {
                            Some(scope) => {
                                format!("allowed when asked, from now on: {}", scope.label)
                            }
                            None => "allowed when asked, this once".into(),
                        };
                        if let Some(scope) = scope {
                            self.rules
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .allow
                                .extend(scope.rules.iter().cloned());
                            self.granted
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .push((scope.label.clone(), scope.rules.clone()));
                        }
                        true
                    }
                    Answer::Deny => {
                        reason = format!("refused when asked ({reason})");
                        false
                    }
                }
            }
        };
        self.record(Judgement {
            tool: tool.to_owned(),
            target,
            allowed,
            reason: reason.clone(),
        });
        // An arena's model is never given the Keychain, whatever rule of the owner's let its command run.
        if let (true, Some(program), Access::Bash { command, .. }, false) =
            (allowed, keychain, access, self.is_contained())
        {
            self.keychain_given
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push((command.clone(), program));
        }
        if allowed {
            Ok(())
        } else {
            Err(format!("not permitted: {reason}"))
        }
    }

    /// What the rules, the narrowings and the mode say of `access` before anyone is asked: a refusal by a narrowing is
    /// recorded and returned as `Err`; every other word, a refusal too, comes back to be settled.
    fn weigh(&self, tool: &str, access: &Access, target: &str) -> Result<Weighed, String> {
        // C3: an `allowed-tools` list only narrows. What one of them does not name is refused here, before any
        // rule or mode is even asked; what all of them name goes on to the normal judgement below, where such a
        // rule never allows anything by itself.
        let agent = agent_narrowing();
        let role = self.role_narrowing();
        if let Some(narrowing) = self
            .narrowings
            .iter()
            .chain(agent.as_deref())
            .chain(role.as_deref())
            .find(|narrowing| !restrict_allows(&narrowing.rules, tool, access))
        {
            let rules = narrowing
                .rules
                .iter()
                .map(Rule::text)
                .collect::<Vec<_>>()
                .join(", ");
            let reason = format!("{} allows only: {rules}", narrowing.by);
            self.record(Judgement {
                tool: tool.to_owned(),
                target: target.to_owned(),
                allowed: false,
                reason: reason.clone(),
            });
            return Err(format!("not permitted: {reason}"));
        }
        // T36c: a role that may edit nothing runs only commands that only read — `git diff --output=…` is a write
        // a prefix rule like `Bash(git diff:*)` would let through.
        if let (Some(role), Access::Bash { command, .. }) = (&role, access)
            && !role.rules.iter().any(|rule| rule.kind == RuleKind::Edit)
            && !kinds::reads_only(&analyze(command))
        {
            let reason = format!("{} only reads: this command is no plain reading", role.by);
            self.record(Judgement {
                tool: tool.to_owned(),
                target: target.to_owned(),
                allowed: false,
                reason: reason.clone(),
            });
            return Err(format!("not permitted: {reason}"));
        }
        let keychain = self.wants_keychain(access);
        let said = {
            let rules = self
                .rules
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            // T74b: the Keychain comes by the owner's word alone — a project's allow rule lets the command run, but
            // not with it open, so then it is asked.
            if keychain.is_some() {
                rules
                    .owners()
                    .verdict_with(tool, access, true, &self.beside())
            } else {
                rules.verdict_with(tool, access, true, &self.beside())
            }
        };
        let mode = format!("mode {}", self.mode_name());
        let mut judgeable = false;
        let (mut verdict, mut reason) = match said {
            Said::Verdict(verdict, why) => (verdict, why),
            // T74: no reader, no `auto` — the owner's word or a rule of theirs.
            _ if keychain.is_some() => (
                Verdict::Ask,
                format!(
                    "{} would get the Keychain: asked in every mode",
                    keychain.as_deref().unwrap_or_default()
                ),
            ),
            unsure_or_silent => match self.command_kind(access) {
                Some(judged) => judged,
                None => {
                    // T52: only a reader a loop or a variable kept from the list, and never where a Read rule of
                    // the owner's holds a file back — a loop's names are no paths a rule could weigh.
                    judgeable = match access {
                        Access::Bash {
                            command,
                            sandboxed: true,
                            ..
                        } => {
                            self.mode() != Mode::DontAsk
                                && kinds::judgeable(&analyze(command))
                                && !self.reads_ruled()
                        }
                        _ => false,
                    };
                    match unsure_or_silent {
                        Said::Unsure(why) => (Verdict::Ask, why),
                        _ => (self.mode().fallback(access), mode.clone()),
                    }
                }
            },
        };
        // Plan mode writes nothing, whatever a rule says (P3) — neither with the file tools nor by a redirection.
        let writes = match access {
            Access::Edit { .. } | Access::Memory { .. } | Access::GlutFile { .. } => true,
            Access::Bash { command, .. } => !analyze(command).writes.is_empty(),
            _ => false,
        };
        if self.mode() == Mode::Plan && writes {
            (verdict, reason) = (Verdict::Deny, mode.clone());
        }
        // T37b: Glut's own files are asked every time — no allow rule stands in for the owner's look at a hook.
        if verdict == Verdict::Allow && matches!(access, Access::GlutFile { .. }) {
            (verdict, reason) = (
                Verdict::Ask,
                format!("{reason}; Glut's own files are always asked"),
            );
        }
        // A message to another pane is asked in plan mode too (T36c): it writes nothing here.
        let plan_asks = self.mode() == Mode::Plan && matches!(access, Access::Pane { .. });
        if verdict == Verdict::Ask && self.mode().asks_nothing() && !plan_asks {
            verdict = Verdict::Deny;
            reason = format!("{reason}; {mode} asks nothing");
        }
        Ok(Weighed {
            verdict,
            reason,
            keychain,
            judgeable,
        })
    }

    /// Judges several changes of one call as one (T62: a rename across files): each weighed as its own call would be,
    /// one refused refusing them all unasked; where any would be asked, the owner is asked once about `shown`, only
    /// this once — a rename's set of files is no rule to keep. Every change is recorded with the outcome.
    pub async fn judge_all(
        &self,
        tool: &str,
        accesses: &[Access],
        shown: &str,
    ) -> Result<(), String> {
        self.judge_all_showing(tool, accesses, shown, None).await
    }

    /// [`Self::judge_all`] with what its question may show beside the target (T91: a rename's files as diffs).
    pub async fn judge_all_showing(
        &self,
        tool: &str,
        accesses: &[Access],
        shown: &str,
        arguments: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let mut weighed = Vec::new();
        for access in accesses {
            let target = describe(access);
            let said = self.weigh(tool, access, &target)?;
            weighed.push((target, said));
        }
        if let Some((target, said)) = weighed
            .iter()
            .find(|(_, said)| said.verdict == Verdict::Deny)
        {
            self.record(Judgement {
                tool: tool.to_owned(),
                target: target.clone(),
                allowed: false,
                reason: said.reason.clone(),
            });
            return Err(format!("not permitted: {}", said.reason));
        }
        let asked = if !weighed.iter().any(|(_, said)| said.verdict == Verdict::Ask) {
            None
        } else if !self.present() {
            Some((false, self.nobody().to_owned()))
        } else {
            Some(
                match self
                    .ask_owner_showing(tool, shown, Vec::new(), arguments)
                    .await
                {
                    Answer::Deny => (false, "refused when asked".to_owned()),
                    _ => (true, "allowed when asked, this once".to_owned()),
                },
            )
        };
        let allowed = asked.as_ref().is_none_or(|(allowed, _)| *allowed);
        for (target, said) in weighed {
            self.record(Judgement {
                tool: tool.to_owned(),
                target,
                allowed,
                reason: asked
                    .as_ref()
                    .map_or(said.reason, |(_, reason)| reason.clone()),
            });
        }
        match asked {
            Some((false, reason)) => Err(format!("not permitted: {reason}")),
            _ => Ok(()),
        }
    }

    /// What a sandboxed command's kind says where no rule does (TUI1e): one that only reads in the working folder runs
    /// in every mode, as a `read_file` would — unless a rule holds back a file it names —; in the `auto` mode
    /// everything runs but a heavy command or one naming a protected path, which are asked. `None`: the rules'
    /// unsureness or the mode decide — always without a sandbox, and in `dont_ask`.
    fn command_kind(&self, access: &Access) -> Option<(Verdict, String)> {
        let Access::Bash {
            command,
            folder,
            sandboxed: true,
            ..
        } = access
        else {
            return None;
        };
        // A headless run that asks nothing keeps to its rules alone.
        if self.mode() == Mode::DontAsk {
            return None;
        }
        let analysis = analyze(command);
        if kinds::reads_only(&analysis) {
            let beside = self.beside();
            let beyond = {
                let rules = self
                    .rules
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                analysis.parts.iter().find_map(|part| {
                    rules.reads_beyond(&part.words, &analysis.reads, folder, &beside)
                })
            };
            return Some(match beyond {
                // Outside the folder auto trusts the sandbox; the other modes ask, as `read_file` cannot go there. A
                // file a rule holds back is asked in every mode.
                Some(READS_OUTSIDE) if self.mode() == Mode::Auto => {
                    (Verdict::Allow, "mode auto".into())
                }
                Some(why) => (Verdict::Ask, why.into()),
                None => (Verdict::Allow, "a command that only reads".into()),
            });
        }
        if self.mode() != Mode::Auto {
            return None;
        }
        // The sandbox lets a write to a protected name below the folder's top through (`sub/.cargo/config.toml`):
        // code the owner's next build or editor would run. And what the lexer cannot read may hide a heavy command
        // — its plain words are searched for one.
        let hidden = analysis
            .is_opaque()
            .then(|| kinds::heavy_in_text(command))
            .flatten();
        let protected = self.names_protected_here(&analysis);
        Some(match kinds::heavy(&analysis).or(hidden) {
            // T61: what the sandbox holds to the arena's own folder runs; a protected name stays as it was.
            Some(why)
                if self.is_confined()
                    && !protected
                    && kinds::reaches_out(&analysis, command).is_none() =>
            {
                (Verdict::Allow, format!("{why}, in the arena's own folder"))
            }
            Some(why) => (Verdict::Ask, format!("{why}: asked even in auto mode")),
            None if protected => (
                Verdict::Ask,
                "the command names a protected path: asked even in auto mode".into(),
            ),
            None => (Verdict::Allow, "mode auto".into()),
        })
    }

    /// The judgements since the last call, oldest first — the caller's own: while subagents run at once (TUI T40),
    /// each takes only what was judged for its calls, the run itself only what was judged outside them. A judgement
    /// made on a task of its own (the sandbox's proxy, for a command's connection) names no subagent; the run waits
    /// for its subagents meanwhile, so such a one is a subagent's, and the first to look takes it — never the run's.
    pub fn take_log(&self) -> Vec<Judgement> {
        let mine = subagent().map(|(id, _)| id);
        let mut log = self.log.lock().unwrap_or_else(|poison| poison.into_inner());
        let (taken, left) = std::mem::take(&mut *log)
            .into_iter()
            .partition(|(by, _)| *by == mine || (mine.is_some() && by.is_none()));
        *log = left;
        taken.into_iter().map(|(_, judged)| judged).collect()
    }

    /// One judgement into the log, under the subagent it was made for.
    fn record(&self, judged: Judgement) {
        let by = subagent().map(|(id, _)| id);
        self.log
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push((by, judged));
    }
}

/// How the reason begins where a second model let a command run as only reading (T52): a host marks such a call.
pub const JUDGED_READING: &str = "judged to only read by";

/// Why a reading command is not free in every mode: it reads out of the working folder.
const READS_OUTSIDE: &str = "the command reads outside the working folder";

/// The rules and mode a caller adds to the settings files: flags (P7) and a role's `permissions` (P1).
#[derive(Debug, Clone, Default)]
pub struct GateOptions {
    /// The mode from a flag; before every file's.
    pub mode: Option<Mode>,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
    /// The project's `.glut/` files count (P5).
    pub trusted: bool,
}

/// What [`build_gate`] makes of the settings files and a caller's options.
pub struct BuiltGate {
    /// The settings' and the options' rules under the mode they name.
    pub gate: Gate,
    /// The project's allow rules or mode were left out: the folder is not trusted.
    pub untrusted_project: bool,
    /// What the settings say about the sandbox (HS3b).
    pub sandbox: settings::SandboxSettings,
    /// The owner's hooks (HS7, B1 — the only ones that ever run).
    pub hooks: crate::hooks::Hooks,
    /// The project has hooks of its own that were parsed but not taken over (B1).
    pub project_hooks_skipped: bool,
    /// The owner's `effort` from `~/.glut/settings.json` (Glut effort, E1): the run's unless a flag, a skill or a
    /// command names one.
    pub effort: Option<crate::config::Effort>,
    /// The owner's refusal limits from `~/.glut/settings.json` (#112, R6); the defaults without.
    pub refusals: crate::config::RefusalLimits,
    /// Whether runs compact themselves (#111, K6) and the context windows the owner named (K2).
    pub compact_auto: bool,
    pub context_windows: std::collections::HashMap<String, u32>,
    /// The owner's prices by model id (`docs/plans/glut-cards.md`, C6), for a budget in money.
    pub prices: std::collections::HashMap<String, crate::config::Prices>,
    /// How many subagents of one reply run at once (TUI T40), from `~/.glut/settings.json`.
    pub parallel_subagents: u32,
    /// From how many bytes a project's guide is pointed out as too large (TUI T42), from `~/.glut/settings.json`.
    pub guide_warn_bytes: u64,
    /// When the terminal UI tells the owner a turn ended or asks (TUI T53), from `~/.glut/settings.json`.
    pub notify: settings::NotifySection,
    /// The owner's `loop` section (TUI T60a): where a loop ends by itself.
    pub loops: settings::LoopSection,
    /// Whether runs ask their model once to go on (T73).
    pub nudge: bool,
    /// Whether the language servers serve the runs (T62).
    pub lsp: bool,
    /// The owner's `maxTurns` (T73b).
    pub max_turns: Option<u32>,
}

/// The note for a project whose own allow rules or mode are left out (a host may say it its own way, with its
/// command to trust the folder).
pub const UNTRUSTED_NOTE: &str =
    "the project's .glut/ allow rules and mode are left out: this folder is not trusted";

impl BuiltGate {
    /// The run's compaction for `model`: the owner's switch and the window the settings name for it.
    pub fn compaction(&self, model: &str) -> crate::config::Compaction {
        crate::config::Compaction {
            auto: self.compact_auto,
            context_window: self.context_windows.get(model).copied(),
        }
    }

    /// What `model` costs, as the owner's settings say.
    pub fn prices(&self, model: &str) -> Option<crate::config::Prices> {
        self.prices.get(model).copied()
    }

    /// What the person starting the run should be told about how it is guarded, one line each: what of the
    /// project's settings was left out, and a sandbox that is off or missing.
    pub fn notes(&self) -> Vec<&'static str> {
        let mut notes = Vec::new();
        if self.untrusted_project {
            notes.push(UNTRUSTED_NOTE);
        }
        if self.project_hooks_skipped {
            notes.push(
                "the project's own .glut/ hooks are not run: only ~/.glut/settings.json hooks run",
            );
        }
        if !self.sandbox.enabled {
            notes.push("commands run without a sandbox: only the permission rules hold them");
        } else if let Some(why) = crate::sandbox::unavailable() {
            // Why, and what helps (T59: Linux has more ways to lack one) — the same for the whole process.
            static NOTE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            notes.push(NOTE.get_or_init(|| {
                format!(
                    "commands run without a sandbox: only the permission rules hold them — {why}"
                )
            }));
        }
        notes
    }
}

/// The gate of a run in `project`: the owner's settings below `home`, the project's if trusted, then `options`
/// — and beside it what else the settings say ([`BuiltGate`]).
///
/// # Errors
///
/// [`crate::HarnessError::Settings`] for a settings file or a flag that does not parse.
pub fn build_gate(
    home: &std::path::Path,
    project: &std::path::Path,
    options: GateOptions,
    asker: Arc<dyn Asker>,
) -> Result<BuiltGate, crate::HarnessError> {
    let loaded = settings::load(home, project, options.trusted)?;
    let mut rules = loaded.rules;
    let flag = |name: &str, list: &[String]| -> Result<Vec<Rule>, crate::HarnessError> {
        list.iter()
            .map(|text| {
                Rule::parse(text).map_err(|reason| crate::HarnessError::Settings {
                    path: format!("--{name}"),
                    reason,
                })
            })
            .collect()
    };
    rules.extend(RuleSet {
        allow: flag("allow", &options.allow)?,
        ask: flag("ask", &options.ask)?,
        deny: flag("deny", &options.deny)?,
    });
    let mode = options.mode.or(loaded.mode).unwrap_or_default();
    Ok(BuiltGate {
        gate: Gate::new(rules, mode, asker),
        untrusted_project: loaded.untrusted_project,
        sandbox: loaded.sandbox,
        hooks: loaded.hooks,
        project_hooks_skipped: loaded.project_hooks_skipped,
        effort: loaded.effort,
        refusals: loaded.refusals,
        compact_auto: loaded.compact_auto,
        context_windows: loaded.context_windows,
        prices: loaded.prices,
        parallel_subagents: loaded.parallel_subagents,
        guide_warn_bytes: loaded.guide_warn_bytes,
        notify: loaded.notify,
        loops: loaded.loops,
        nudge: loaded.nudge,
        lsp: loaded.lsp,
        max_turns: loaded.max_turns,
    })
}

/// One reach an allowance from then on may have: the rules it adds, and how the owner reads it.
#[derive(Debug, Clone)]
struct Scope {
    label: String,
    rules: Vec<Rule>,
}

/// What the owner may allow from then on for this call, narrowest first. A command offers every reach of
/// [`kinds::reaches`] — exactly it, its sub-command (`git status *`), its program (`git *`) — for each of its parts
/// at once (`cargo fmt *, cargo test *`); one the lexer cannot read offers none, it would be asked again anyway. A
/// path, a host or a tool offers exactly itself.
/// The scopes an "always" may take; `keychain`: the command would get the Keychain (T74b), so never a whole program
/// (`gh *`) — at most a program and its sub-command.
fn scopes(tool: &str, access: &Access, keychain: bool) -> Vec<Scope> {
    let one = |text: String| {
        Rule::parse(&text)
            .ok()
            .map(|rule| {
                vec![Scope {
                    label: describe(access),
                    rules: vec![rule],
                }]
            })
            .unwrap_or_default()
    };
    match access {
        Access::Bash { command, .. } => {
            let analysis = analyze(command);
            if analysis.is_opaque()
                || analysis
                    .parts
                    .iter()
                    .any(|part| !part.assignments.is_empty())
            {
                return Vec::new();
            }
            // A part that only reads runs without a rule beside one that does more: it is left out of what the owner
            // chooses from. A command that only reads (asked for a file a rule holds back) keeps every part.
            let doing: Vec<_> = analysis
                .parts
                .iter()
                .filter(|part| !kinds::reader(&part.words))
                .collect();
            let parts = if doing.is_empty() {
                analysis.parts.iter().collect()
            } else {
                doing
            };
            let per_part: Vec<Vec<kinds::Reach>> = parts.into_iter().map(kinds::reaches).collect();
            if per_part.is_empty() || per_part.iter().any(Vec::is_empty) {
                return Vec::new();
            }
            let mut out: Vec<Scope> = Vec::new();
            // Tier by tier, each part at its widest reach no wider than the tier: `cargo fmt, cargo test *`.
            let widest = if keychain { 1 } else { 2 };
            for tier in 0..=widest {
                let mut reaches: Vec<&kinds::Reach> = per_part
                    .iter()
                    .filter_map(|reaches| reaches.iter().rfind(|reach| reach.tier() <= tier))
                    .collect();
                reaches.dedup();
                let Some(rules) = reaches
                    .iter()
                    .map(|reach| Rule::parse(&reach.rule()).ok())
                    .collect::<Option<Vec<_>>>()
                else {
                    return out;
                };
                let label = reaches
                    .iter()
                    .map(|reach| reach.label())
                    .collect::<Vec<_>>()
                    .join(", ");
                if out.last().is_none_or(|last| last.label != label) {
                    out.push(Scope { label, rules });
                }
            }
            out
        }
        Access::Edit { path } => one(format!("Edit({})", glob_escape(&normal_path(path)))),
        Access::Read { path } => one(format!("Read({})", glob_escape(&normal_path(path)))),
        Access::Network { host } => one(format!("Network({host})")),
        // An MCP call's allowance is the tool's own rule, which names it exactly (M5).
        Access::Mcp { server, tool } => one(format!("mcp__{server}__{tool}")),
        Access::Own
        | Access::Other
        | Access::Memory { .. }
        | Access::Agent { .. }
        | Access::Pane { .. } => one(tool.to_owned()),
        // T37b: this once only — no "always" for a write of a skill, an agent or a hook.
        Access::GlutFile { .. } => Vec::new(),
    }
}

/// A path as a glob that matches only itself.
fn glob_escape(path: &str) -> String {
    path.chars()
        .flat_map(|c| match c {
            '*' | '?' | '[' | ']' | '{' | '}' | '\\' => vec!['[', c, ']'],
            c => vec![c],
        })
        .collect()
}

/// What a call would do, on one line, for an ask and the events.
fn describe(access: &Access) -> String {
    match access {
        Access::Read { path } | Access::Edit { path } => normal_path(path),
        Access::Bash { command, .. } => command.clone(),
        Access::Network { host } => host.clone(),
        // For an ask and the events: server and tool, the way the rule names them (M5).
        Access::Mcp { server, tool } => format!("{server}/{tool}"),
        Access::Memory { change }
        | Access::Agent { what: change }
        | Access::Pane { what: change }
        | Access::GlutFile { change } => change.clone(),
        Access::Own | Access::Other => String::new(),
    }
}

/// Whether a command's `allowed-tools` rules name this call (HS4, C3): the matching the allow rules get, on a set
/// with nothing in it but these rules — so they can only ever say "that call is in the list", never allow it. A
/// compound command is named only when every part of it is, a write only when an `Edit` rule names it too.
fn restrict_allows(rules: &[Rule], tool: &str, access: &Access) -> bool {
    let only = RuleSet {
        allow: rules.to_vec(),
        ask: Vec::new(),
        deny: Vec::new(),
    };
    matches!(
        only.verdict_with(tool, access, false, &Folders::default()),
        Said::Verdict(Verdict::Allow, _)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(allow: &[&str], ask: &[&str], deny: &[&str]) -> RuleSet {
        let parse = |list: &[&str]| list.iter().map(|text| Rule::parse(text).unwrap()).collect();
        RuleSet {
            allow: parse(allow),
            ask: parse(ask),
            deny: parse(deny),
        }
    }

    fn bash(command: &str) -> Access {
        Access::Bash {
            command: command.into(),
            folder: String::new(),
            unsandboxed: false,
            sandboxed: true,
        }
    }

    /// The verdict alone; a call the rules cannot read counts as asked.
    fn said(set: &RuleSet, tool: &str, access: Access) -> Option<Verdict> {
        match set.verdict(tool, &access) {
            Said::Verdict(verdict, _) => Some(verdict),
            Said::Unsure(_) => Some(Verdict::Ask),
            Said::Silent => None,
        }
    }

    #[test]
    fn rules_parse_in_claude_codes_format() {
        assert!(Rule::parse("Bash(cargo test:*)").is_ok());
        assert!(Rule::parse("Read").is_ok());
        assert!(Rule::parse("Edit(src/**/*.rs)").is_ok());
        for bad in ["", "Bash(cargo", "(x)", "Edit([)", "Bash(:*)", "Bash( :*)"] {
            assert!(Rule::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn names_tool_is_a_plain_tool_rule_without_a_pattern() {
        // HS8, D10: what the core asks of a skill's `allowed_tools` to decide whether `task` is offered.
        assert!(Rule::parse("task").unwrap().names_tool("task"));
        assert!(!Rule::parse("task").unwrap().names_tool("other"));
        assert!(
            !Rule::parse("task(x)").unwrap().names_tool("task"),
            "a patterned rule never matches a bare tool call"
        );
        assert!(!Rule::parse("Read").unwrap().names_tool("task"));
        assert!(!Rule::parse("read_file").unwrap().names_tool("task"));
        assert!(Rule::parse("read_file").unwrap().names_tool("read_file"));
    }

    #[test]
    fn a_prefix_rule_takes_the_command_and_its_arguments_only() {
        let set = rules(&["Bash(cargo test:*)", "Bash(git status)"], &[], &[]);
        assert_eq!(said(&set, "bash", bash("cargo test")), Some(Verdict::Allow));
        assert_eq!(
            said(&set, "bash", bash("cargo test --all")),
            Some(Verdict::Allow)
        );
        assert_eq!(said(&set, "bash", bash("cargo testx")), None);
        assert_eq!(said(&set, "bash", bash("git status")), Some(Verdict::Allow));
        assert_eq!(
            said(&set, "bash", bash("git status -s")),
            None,
            "an exact rule is exact"
        );
    }

    #[test]
    fn a_compound_command_is_allowed_only_part_by_part_and_denied_by_any_part() {
        let set = rules(
            &["Bash(cargo test:*)", "Bash(git status)"],
            &[],
            &["Bash(git push:*)", "Bash(git * push *)"],
        );
        assert_eq!(
            said(&set, "bash", bash("cargo test && git status")),
            Some(Verdict::Allow)
        );
        assert_eq!(said(&set, "bash", bash("cargo test; rm -rf x")), None);
        assert_eq!(said(&set, "bash", bash("cargo test | tee log")), None);
        assert_eq!(
            said(&set, "bash", bash("cargo test && git push origin")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(&set, "bash", bash("git -C . push origin main")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(&set, "bash", bash("cargo test & git push")),
            Some(Verdict::Deny)
        );
        // Separators inside quotes are text.
        assert_eq!(
            said(&set, "bash", bash("cargo test -- 'a;b && c'")),
            Some(Verdict::Allow)
        );
    }

    #[test]
    fn a_substituted_command_is_never_allowed_by_a_rule() {
        let set = rules(&["Bash(cargo test:*)", "Bash(echo:*)"], &[], &[]);
        assert_eq!(
            said(&set, "bash", bash("cargo test $(rm -rf x)")),
            Some(Verdict::Ask)
        );
        assert_eq!(
            said(&set, "bash", bash("echo `whoami`")),
            Some(Verdict::Ask)
        );
        assert_eq!(
            said(&set, "bash", bash("echo <(cat secret)")),
            Some(Verdict::Ask)
        );
        assert_eq!(
            said(&set, "bash", bash("echo '$(not run)'")),
            Some(Verdict::Allow),
            "single quotes are text"
        );
    }

    #[test]
    fn deny_beats_ask_beats_allow() {
        let set = rules(&["Bash"], &["Bash(npm publish:*)"], &["Bash(rm -rf:*)"]);
        assert_eq!(said(&set, "bash", bash("ls")), Some(Verdict::Allow));
        assert_eq!(said(&set, "bash", bash("npm publish")), Some(Verdict::Ask));
        assert_eq!(said(&set, "bash", bash("rm -rf /")), Some(Verdict::Deny));
    }

    #[test]
    fn path_rules_cover_their_tools() {
        let set = rules(
            &["Edit(src/**)"],
            &[],
            &["Read(**/.env)", "Edit(Cargo.lock)"],
        );
        let edit = |path: &str| Access::Edit { path: path.into() };
        assert_eq!(
            said(&set, "write_file", edit("src/a/b.rs")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&set, "edit_file", edit("./src/main.rs")),
            Some(Verdict::Allow)
        );
        assert_eq!(said(&set, "write_file", edit("README.md")), None);
        assert_eq!(
            said(&set, "edit_file", edit("Cargo.lock")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(
                &set,
                "read_file",
                Access::Read {
                    path: "config/.env".into()
                }
            ),
            Some(Verdict::Deny)
        );
        let by_name = rules(&["grep"], &[], &[]);
        assert_eq!(
            said(&by_name, "grep", Access::Read { path: ".".into() }),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&by_name, "glob", Access::Read { path: ".".into() }),
            None
        );
    }

    #[test]
    fn the_mode_decides_where_no_rule_speaks() {
        let read = Access::Read { path: "a".into() };
        let edit = Access::Edit { path: "a".into() };
        let cmd = bash("ls");
        assert_eq!(Mode::Default.fallback(&read), Verdict::Allow);
        assert_eq!(Mode::Default.fallback(&edit), Verdict::Ask);
        assert_eq!(Mode::Default.fallback(&cmd), Verdict::Ask);
        assert_eq!(Mode::AcceptEdits.fallback(&edit), Verdict::Allow);
        assert_eq!(Mode::AcceptEdits.fallback(&cmd), Verdict::Ask);
        assert_eq!(Mode::Plan.fallback(&edit), Verdict::Deny);
        assert_eq!(Mode::Plan.fallback(&read), Verdict::Allow);
        assert_eq!(Mode::DontAsk.fallback(&cmd), Verdict::Deny);
        assert_eq!(Mode::Auto.fallback(&cmd), Verdict::Allow);
        assert_eq!(Mode::parse("acceptEdits"), Some(Mode::AcceptEdits));
        assert_eq!(Mode::parse("bypassPermissions"), None, "there is no bypass");
    }

    /// Answers every ask with the same answer and counts the asks.
    struct Always(Answer, Mutex<u32>);

    impl Asker for Always {
        fn ask<'a>(&'a self, _request: &'a AskRequest) -> AnswerFuture<'a> {
            *self.1.lock().unwrap() += 1;
            let answer = self.0;
            Box::pin(async move { answer })
        }
    }

    /// Answers yes and keeps who asked.
    struct Who(Mutex<Vec<Option<String>>>);

    impl Asker for Who {
        fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a> {
            self.0.lock().unwrap().push(request.from.clone());
            Box::pin(async { Answer::Allow })
        }
    }

    /// Answers yes and keeps what its gate held as the asked call's arguments while it asked (T91).
    struct SeesArguments(
        std::sync::OnceLock<std::sync::Weak<Gate>>,
        Mutex<Vec<Option<serde_json::Value>>>,
    );

    impl Asker for SeesArguments {
        fn ask<'a>(&'a self, _request: &'a AskRequest) -> AnswerFuture<'a> {
            let held = self
                .0
                .get()
                .and_then(std::sync::Weak::upgrade)
                .and_then(|gate| gate.asked_arguments());
            self.1.lock().unwrap().push(held);
            Box::pin(async { Answer::Allow })
        }
    }

    #[tokio::test]
    async fn a_questions_whole_arguments_are_held_only_while_it_is_open() {
        let asker = Arc::new(SeesArguments(
            std::sync::OnceLock::new(),
            Mutex::new(Vec::new()),
        ));
        let gate = Arc::new(Gate::new(RuleSet::default(), Mode::Default, asker.clone()));
        asker.0.set(Arc::downgrade(&gate)).unwrap();
        let long = serde_json::json!({"path": "a.txt", "content": "x".repeat(5_000)});
        let edit = Access::Edit {
            path: "a.txt".into(),
        };
        assert!(
            gate.judge_call("write_file", &edit, Some(&long))
                .await
                .is_ok()
        );
        assert_eq!(asker.1.lock().unwrap().pop(), Some(Some(long)));
        assert_eq!(gate.asked_arguments(), None, "gone once answered");
        assert!(gate.judge("write_file", &edit).await.is_ok());
        assert_eq!(asker.1.lock().unwrap().pop(), Some(None));
    }

    /// Answers yes and keeps whether each ask was for the Keychain.
    struct KeychainAsks(Mutex<Vec<bool>>);

    impl Asker for KeychainAsks {
        fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a> {
            self.0.lock().unwrap().push(request.keychain);
            Box::pin(async { Answer::Allow })
        }
    }

    /// Says READS for the commands it is given, CHANGES for the rest, and counts its judgements (T52).
    struct Reads(Vec<&'static str>, Mutex<u32>);

    impl Judge for Reads {
        fn name(&self) -> String {
            "judge".into()
        }

        fn judge<'a>(&'a self, command: &'a str, _folder: &'a str) -> OpinionFuture<'a> {
            *self.1.lock().unwrap() += 1;
            let reads_only = self.0.contains(&command);
            Box::pin(async move {
                Some(Opinion {
                    reads_only,
                    why: if reads_only { "counts" } else { "writes" }.into(),
                    by: "judge".into(),
                })
            })
        }
    }

    /// Answers yes and keeps each ask's note.
    struct Notes(Mutex<Vec<Option<String>>>);

    impl Asker for Notes {
        fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a> {
            self.0.lock().unwrap().push(request.note.clone());
            Box::pin(async { Answer::Allow })
        }
    }

    #[tokio::test]
    async fn a_second_model_lets_only_what_reads_run_and_never_over_a_rule_or_a_heavy_command() {
        let looped = "for f in *.md; do grep -c x \"$f\"; done";
        let asker = Arc::new(Notes(Mutex::new(Vec::new())));
        let gate = Gate::new(
            rules(&[], &["Bash(make *)"], &[]),
            Mode::Default,
            asker.clone(),
        );
        let judge = Arc::new(Reads(vec![looped, "make test"], Mutex::new(0)));
        gate.set_judge(Some(judge.clone()));
        assert!(gate.judge("bash", &bash(looped)).await.is_ok());
        assert!(asker.0.lock().unwrap().is_empty(), "it read: nobody asked");
        assert!(
            gate.take_log()[0].reason.starts_with(JUDGED_READING),
            "said as the judge's"
        );
        assert!(gate.judge("bash", &bash(looped)).await.is_ok());
        assert_eq!(
            *judge.1.lock().unwrap(),
            1,
            "the same command is judged once"
        );
        assert!(
            gate.judge("bash", &bash("for f in *.txt; do wc -l \"$f\"; done"))
                .await
                .is_ok()
        );
        assert_eq!(
            asker.0.lock().unwrap().pop(),
            Some(Some("judge: writes".into())),
            "asked, the judge's word beside it"
        );
        assert!(gate.judge("bash", &bash("touch a; ls")).await.is_ok());
        assert_eq!(
            asker.0.lock().unwrap().pop(),
            Some(None),
            "no reader: never judged"
        );
        assert!(gate.judge("bash", &bash("make test")).await.is_ok());
        assert_eq!(
            asker.0.lock().unwrap().pop(),
            Some(None),
            "an ask rule of the owner's is asked, never judged"
        );
        // Nobody there: what reads runs, the rest stays refused.
        let alone = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        alone.set_judge(Some(judge.clone()));
        assert!(alone.judge("bash", &bash(looped)).await.is_ok());
        assert!(
            alone
                .judge("bash", &bash("for f in *.txt; do wc -l \"$f\"; done"))
                .await
                .is_err()
        );
        // In plan mode a write stays refused whatever the judge says.
        let planning = Gate::new(RuleSet::default(), Mode::Plan, asker.clone());
        planning.set_judge(Some(Arc::new(Reads(vec!["echo x > f"], Mutex::new(0)))));
        assert!(planning.judge("bash", &bash("echo x > f")).await.is_err());
    }

    #[tokio::test]
    async fn a_listed_program_is_asked_for_the_keychain_in_every_mode_unless_a_rule_allows_it_or_it_is_open()
     {
        let asker = Arc::new(KeychainAsks(Mutex::new(Vec::new())));
        for mode in [Mode::Default, Mode::AcceptEdits, Mode::Auto] {
            let gate = Gate::new(RuleSet::default(), mode, asker.clone());
            // `gh pr list` only reads, and `auto` runs everything sandboxed — the Keychain is asked all the same.
            assert!(gate.judge("bash", &bash("gh pr list")).await.is_ok());
            assert_eq!(asker.0.lock().unwrap().pop(), Some(true), "{mode:?}");
            assert!(gate.judge("bash", &bash("git status")).await.is_ok());
            assert_eq!(
                asker.0.lock().unwrap().pop(),
                None,
                "{mode:?}: a reader without the Keychain runs"
            );
        }
        // An owner's rule naming the program and its sub-command is the owner's word: no ask.
        let ruled = Gate::new(
            rules(&["Bash(gh pr *)"], &[], &[]),
            Mode::Default,
            asker.clone(),
        );
        assert!(
            ruled
                .judge("bash", &bash("gh pr create -t x"))
                .await
                .is_ok()
        );
        assert!(asker.0.lock().unwrap().is_empty());
        // Nobody there: refused without a rule.
        let headless = Gate::new(RuleSet::default(), Mode::DontAsk, asker.clone());
        assert!(headless.judge("bash", &bash("gh pr list")).await.is_err());
        // Open to every command (the owner's switch): nothing to ask for, `auto` runs it as any other.
        let open = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        let keychain = Keychain::default();
        keychain.set_session_open(true);
        open.set_keychain(keychain);
        assert!(open.judge("bash", &bash("gh pr list")).await.is_ok());
        assert!(asker.0.lock().unwrap().is_empty());
        // Outside the sandbox the Keychain was never shut, and `auto` asks every command: the mode's question, not
        // the Keychain's.
        let mut unfenced = bash("gh pr list");
        if let Access::Bash { sandboxed, .. } = &mut unfenced {
            *sandboxed = false;
        }
        let auto = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(
            auto.judge("bash", &unfenced).await.is_ok(),
            "the asker allows"
        );
        assert_eq!(
            asker.0.lock().unwrap().pop(),
            Some(false),
            "asked, and not for the Keychain"
        );
    }

    #[tokio::test]
    async fn a_projects_allow_rule_runs_a_command_but_never_gives_it_the_keychain() {
        let asker = Arc::new(KeychainAsks(Mutex::new(Vec::new())));
        let project = RuleSet {
            allow: vec![Rule::parse("Bash(gh *)").unwrap().from_project()],
            ..RuleSet::default()
        };
        let gate = Gate::new(project.clone(), Mode::Default, asker.clone());
        assert!(gate.judge("bash", &bash("gh pr list")).await.is_ok());
        assert_eq!(
            asker.0.lock().unwrap().pop(),
            Some(true),
            "the Keychain is the owner's word, asked"
        );
        // Without the Keychain in play the project's rule still allows: `gh` not on the list.
        let unlisted = Gate::new(project, Mode::Default, asker.clone());
        unlisted.set_keychain(Keychain::new(crate::keychain::Owner {
            open: false,
            programs: Some(vec!["docker".into()]),
        }));
        assert!(
            unlisted
                .judge("bash", &bash("gh pr create -t x"))
                .await
                .is_ok()
        );
        assert!(
            asker.0.lock().unwrap().is_empty(),
            "allowed by the project's rule, unasked"
        );
    }

    #[tokio::test]
    async fn the_shell_opens_the_keychain_for_what_the_gate_judged_once_and_never_for_a_mixed_chain()
     {
        let asker = Arc::new(KeychainAsks(Mutex::new(Vec::new())));
        let gate = Gate::new(RuleSet::default(), Mode::Default, asker.clone());
        assert!(gate.judge("bash", &bash("gh pr list")).await.is_ok());
        assert_eq!(gate.keychain_given("gh pr list").as_deref(), Some("gh"));
        assert_eq!(gate.keychain_given("gh pr list"), None, "taken once");
        let chain = "gh pr list && curl -s https://x.invalid";
        let _ = gate.judge("bash", &bash(chain)).await;
        assert_eq!(
            asker.0.lock().unwrap().last(),
            Some(&false),
            "asked as any command, not for the Keychain"
        );
        assert_eq!(gate.keychain_given(chain), None);
        // A Keychain given and not taken goes when the same command is judged again and refused.
        let refusing = Gate::new(
            RuleSet::default(),
            Mode::Default,
            Arc::new(Always(Answer::Deny, Mutex::new(0))),
        );
        refusing
            .keychain_given
            .lock()
            .unwrap()
            .push(("gh pr list".into(), "gh".into()));
        assert!(refusing.judge("bash", &bash("gh pr list")).await.is_err());
        assert_eq!(refusing.keychain_given("gh pr list"), None);
    }

    #[tokio::test]
    async fn only_an_owners_rule_naming_a_program_and_its_sub_command_gives_the_keychain_unasked() {
        let asker = Arc::new(KeychainAsks(Mutex::new(Vec::new())));
        for (rule, asked) in [
            ("Bash(git *)", true),
            ("Bash(gh *)", true),
            ("Bash(git:*)", true),
            ("Bash(gh p*)", true),
            ("Bash(git push *)", false),
            ("Bash(git push:*)", false),
            ("Bash(gh pr *)", false),
        ] {
            let gate = Gate::new(rules(&[rule], &[], &[]), Mode::Default, asker.clone());
            let command = if rule.contains("gh") {
                "gh pr list"
            } else {
                "git push origin main"
            };
            assert!(gate.judge("bash", &bash(command)).await.is_ok(), "{rule}");
            let was = asker.0.lock().unwrap().pop();
            assert_eq!(was, asked.then_some(true), "{rule}");
        }
    }

    #[test]
    fn a_setting_given_to_git_for_one_call_makes_the_command_opaque() {
        assert!(analyze("git -c credential.helper=x push").is_opaque());
        assert!(analyze("git -c color.ui=false log").is_opaque());
        assert!(analyze("git --config-env=core.pager=X log").is_opaque());
        assert!(
            !analyze("git log -c").is_opaque(),
            "a sub-command's own -c is no setting"
        );
    }

    #[test]
    fn a_keychain_ask_offers_no_whole_program() {
        let access = bash("gh pr create -t x");
        let wide: Vec<String> = scopes("bash", &access, false)
            .into_iter()
            .map(|scope| scope.label)
            .collect();
        let narrow: Vec<String> = scopes("bash", &access, true)
            .into_iter()
            .map(|scope| scope.label)
            .collect();
        assert!(wide.len() > narrow.len(), "{wide:?} {narrow:?}");
        assert!(!narrow.is_empty());
        assert!(narrow.iter().all(|label| label != "gh *"), "{narrow:?}");
    }

    #[tokio::test]
    async fn a_memory_change_is_asked_even_in_auto_shows_what_it_does_and_plan_refuses_it_whatever_a_rule_says()
     {
        let change = Access::Memory {
            change: "keep x (3 bytes) in /m".into(),
        };
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let auto = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(auto.judge("memory", &change).await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 1, "asked in auto");
        assert_eq!(auto.take_log()[0].target, "keep x (3 bytes) in /m");
        let allowed = RuleSet {
            allow: vec![Rule::parse("memory").unwrap()],
            ..RuleSet::default()
        };
        let plan = Gate::new(allowed, Mode::Plan, asker.clone());
        assert!(
            plan.judge("memory", &change).await.is_err(),
            "plan mode writes nothing"
        );
        assert!(
            plan.judge("memory", &Access::Own).await.is_ok(),
            "reading stays free"
        );
    }

    #[tokio::test]
    async fn a_glut_file_write_is_asked_in_every_mode_once_only_and_plan_refuses_it() {
        let change = Access::GlutFile {
            change: "write /h/.glut/hooks.json (90 bytes)".into(),
        };
        assert!(
            scopes("glut_file", &change, false).is_empty(),
            "no \"always\" for it"
        );
        // An allow rule does not stand in for the ask.
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let allowing = Gate::new(rules(&["glut_file"], &[], &[]), Mode::Auto, asker.clone());
        assert!(allowing.judge("glut_file", &change).await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 1, "asked despite the rule");
        for mode in [Mode::Default, Mode::AcceptEdits, Mode::Auto] {
            let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
            let gate = Gate::new(RuleSet::default(), mode, asker.clone());
            assert!(gate.judge("glut_file", &change).await.is_ok());
            assert_eq!(*asker.1.lock().unwrap(), 1, "asked in {mode:?}");
        }
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let plan = Gate::new(RuleSet::default(), Mode::Plan, asker);
        assert!(plan.judge("glut_file", &change).await.is_err());
    }

    #[tokio::test]
    async fn a_panes_role_narrows_every_run_of_it_its_subagents_too_and_can_change() {
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let gate = Gate::new(RuleSet::default(), Mode::Auto, asker);
        let read = Access::Read {
            path: "a.rs".into(),
        };
        let edit = Access::Edit {
            path: "a.rs".into(),
        };
        gate.set_role(Some((
            "the role reviewer".into(),
            vec![Rule::parse("Read").unwrap()],
        )));
        assert!(gate.judge("read_file", &read).await.is_ok());
        let refused = gate.judge("edit_file", &edit).await.unwrap_err();
        assert!(
            refused.contains("the role reviewer allows only: Read"),
            "{refused}"
        );
        assert!(!gate.offers("edit_file", &edit));
        // A subagent with no tools of its own is still the reviewer's.
        let theirs = as_agent(
            "t1".into(),
            "look".into(),
            None,
            gate.judge("edit_file", &edit),
        )
        .await;
        assert!(theirs.is_err());
        // Without an Edit rule a role only reads: a prefix rule does not let a writing option through.
        gate.set_role(Some((
            "the role reviewer".into(),
            vec![Rule::parse("Bash(git diff:*)").unwrap()],
        )));
        assert!(gate.judge("bash", &bash("git diff HEAD~1")).await.is_ok());
        let wrote = gate
            .judge("bash", &bash("git diff --output=src/lib.rs"))
            .await
            .unwrap_err();
        assert!(wrote.contains("only reads"), "{wrote}");
        assert!(!gate.role_reaches_network(), "no Network rule, no network");
        assert_eq!(gate.role_rules(), Some(vec!["Bash(git diff:*)".to_owned()]));
        gate.set_role(Some(("the role nothing".into(), Vec::new())));
        assert!(
            gate.judge("read_file", &read).await.is_err(),
            "an empty list is no tool"
        );
        gate.set_role(None);
        assert!(gate.judge("edit_file", &edit).await.is_ok());
    }

    #[tokio::test]
    async fn a_message_to_another_pane_is_asked_even_in_plan_mode_and_says_to_whom_and_what() {
        let told = Access::Pane {
            what: "to pane 2: the parser is done".into(),
        };
        assert_eq!(describe(&told), "to pane 2: the parser is done");
        assert_eq!(
            scopes("tell", &told, false).len(),
            1,
            "an \"always\" for the session"
        );
        for mode in [Mode::Plan, Mode::Default, Mode::AcceptEdits] {
            let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
            let gate = Gate::new(RuleSet::default(), mode, asker.clone());
            assert!(gate.judge("tell", &told).await.is_ok(), "{mode:?}");
            assert_eq!(*asker.1.lock().unwrap(), 1, "asked in {mode:?}");
        }
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let auto = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(auto.judge("tell", &told).await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 0, "auto lets it through");
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let quiet = Gate::new(RuleSet::default(), Mode::DontAsk, asker);
        assert!(
            quiet.judge("tell", &told).await.is_err(),
            "dont-ask asks nothing"
        );
    }

    #[tokio::test]
    async fn a_subagents_judgements_are_its_own_and_its_asks_name_it() {
        let asker = Arc::new(Who(Mutex::new(Vec::new())));
        let gate = Gate::new(RuleSet::default(), Mode::Default, asker.clone());
        gate.judge("read_file", &Access::Read { path: "x".into() })
            .await
            .unwrap();
        let theirs = as_subagent("t1".into(), "find the callers".into(), async {
            gate.judge("bash", &bash("make build")).await.unwrap();
            gate.take_log()
        })
        .await;
        assert_eq!(
            theirs.len(),
            2,
            "what was judged for it, and what names nobody"
        );
        assert_eq!(
            (theirs[0].tool.as_str(), theirs[1].tool.as_str()),
            ("read_file", "bash")
        );
        assert!(gate.take_log().is_empty());
        // Beside another subagent, the other's judgement waits for that one.
        as_subagent("t2".into(), "other".into(), async {
            gate.judge("read_file", &Access::Read { path: "y".into() })
                .await
                .unwrap();
        })
        .await;
        let theirs = as_subagent("t1".into(), "find the callers".into(), async {
            gate.take_log()
        })
        .await;
        assert!(theirs.is_empty(), "t2's is not t1's");
        assert_eq!(gate.take_log().len(), 0, "nor the run's");
        assert_eq!(
            *asker.0.lock().unwrap(),
            [Some("find the callers".to_owned())]
        );
    }

    #[tokio::test]
    async fn an_ask_goes_to_the_asker_and_always_holds_from_then_on() {
        let asker = Arc::new(Always(Answer::AllowAlways, Mutex::new(0)));
        let gate = Gate::new(RuleSet::default(), Mode::Default, asker.clone());
        assert!(gate.judge("bash", &bash("make build")).await.is_ok());
        assert!(gate.judge("bash", &bash("make build")).await.is_ok());
        assert_eq!(
            *asker.1.lock().unwrap(),
            1,
            "asked once, then the run's own rule answered"
        );
        assert!(
            gate.judge("read_file", &Access::Read { path: "x".into() })
                .await
                .is_ok()
        );
        let log = gate.take_log();
        assert_eq!(log.len(), 3);
        assert!(
            log[0].reason.contains("from now on")
                && log[1].reason.contains("rule Bash(make build)")
        );
        assert!(gate.take_log().is_empty());
        assert_eq!(
            gate.granted(),
            [Granted {
                label: "make build".into(),
                rules: vec!["Bash(make build)".into()]
            }]
        );
        let given = ["Bash(make build)".to_owned()];
        assert!(gate.take_back(&given));
        assert!(!gate.take_back(&given), "given once, taken back once");
        assert!(gate.granted().is_empty());
        assert!(gate.judge("bash", &bash("make build")).await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 2, "asked again once taken back");
    }

    #[tokio::test]
    async fn changes_judged_as_one_are_asked_once_and_one_refused_refuses_all_unasked() {
        let edit = |path: &str| Access::Edit { path: path.into() };
        let both = [edit("src/a.rs"), edit("src/b.rs")];
        // Default mode asks about an edit: one question for the two, nothing kept from it.
        let asker = Arc::new(Always(Answer::AllowAlways, Mutex::new(0)));
        let gate = Gate::new(RuleSet::default(), Mode::Default, asker.clone());
        assert!(gate.judge_all("lsp", &both, "rename a → b").await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 1);
        assert!(
            gate.granted().is_empty(),
            "a rename's files are no rule to keep"
        );
        let log = gate.take_log();
        assert_eq!(log.len(), 2);
        assert!(log.iter().all(|judged| judged.allowed));
        // Refused when asked: neither goes.
        let gate = Gate::new(
            RuleSet::default(),
            Mode::Default,
            Arc::new(Always(Answer::Deny, Mutex::new(0))),
        );
        assert!(gate.judge_all("lsp", &both, "rename a → b").await.is_err());
        assert!(gate.take_log().iter().all(|judged| !judged.allowed));
        // A rule refusing one file refuses the whole rename, nobody asked; plan mode writes nothing.
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let rules = RuleSet {
            deny: vec![Rule::parse("Edit(src/b.rs)").unwrap()],
            ..RuleSet::default()
        };
        let gate = Gate::new(rules, Mode::AcceptEdits, asker.clone());
        assert!(gate.judge_all("lsp", &both, "rename a → b").await.is_err());
        let gate = Gate::new(RuleSet::default(), Mode::Plan, asker.clone());
        assert!(gate.judge_all("lsp", &both, "rename a → b").await.is_err());
        assert_eq!(*asker.1.lock().unwrap(), 0);
        // Accepting edits: no question at all.
        let gate = Gate::new(RuleSet::default(), Mode::AcceptEdits, asker.clone());
        assert!(gate.judge_all("lsp", &both, "rename a → b").await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 0);
        // Nobody there to ask: refused.
        let gate = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        assert!(gate.judge_all("lsp", &both, "rename a → b").await.is_err());
    }

    #[tokio::test]
    async fn taking_an_always_back_leaves_the_same_rule_of_the_settings() {
        let asker = Arc::new(Always(Answer::AllowAlways, Mutex::new(0)));
        let rules = RuleSet {
            allow: vec![Rule::parse("Bash(make build)").unwrap()],
            ..RuleSet::default()
        };
        let gate = Gate::new(rules, Mode::Default, asker.clone());
        assert!(gate.judge("bash", &bash("make test")).await.is_ok());
        assert!(gate.take_back(&["Bash(make test)".to_owned()]));
        assert_eq!(
            gate.rules()
                .allow
                .iter()
                .map(Rule::text)
                .collect::<Vec<_>>(),
            ["Bash(make build)"]
        );
    }

    #[tokio::test]
    async fn headless_asks_are_refused_and_own_state_is_free() {
        let gate = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        let refused = gate.judge("bash", &bash("make")).await.unwrap_err();
        assert!(refused.contains("no one to ask"), "{refused}");
        assert!(gate.judge("kill_shell", &Access::Own).await.is_ok());
        let plan = Gate::new(
            rules(&["Bash(git log:*)"], &[], &[]),
            Mode::Plan,
            Arc::new(Nobody),
        );
        assert!(
            plan.judge("bash", &bash("git log -3")).await.is_ok(),
            "an allow rule lets a command through"
        );
        assert!(
            plan.judge("write_file", &Access::Edit { path: "a".into() })
                .await
                .is_err()
        );
    }

    /// The first rule an "always" adds.
    fn always_rule(tool: &str, access: &Access) -> Option<Rule> {
        scopes(tool, access, false)
            .into_iter()
            .next()?
            .rules
            .into_iter()
            .next()
    }

    #[test]
    fn an_always_rule_matches_exactly_what_was_asked() {
        let rule = always_rule("bash", &bash("make build")).unwrap();
        assert!(rule.matches_words(&analyze("make build").parts[0].words));
        assert!(
            !rule.matches_words(&analyze("make build -j4").parts[0].words),
            "exactly this command"
        );
        assert!(always_rule("bash", &bash("rm *")).is_none());
        let path = always_rule(
            "write_file",
            &Access::Edit {
                path: "src/[a].rs".into(),
            },
        )
        .unwrap();
        assert!(path.matches_path(
            "write_file",
            &Access::Edit {
                path: "src/[a].rs".into()
            }
        ));
        assert!(!path.matches_path(
            "write_file",
            &Access::Edit {
                path: "src/a.rs".into()
            }
        ));
    }

    #[test]
    fn wildcards_match_runs_of_characters() {
        assert!(wildcard("git * push *", "git -C . push origin"));
        assert!(!wildcard("git * push *", "git push origin"));
        assert!(wildcard("*", "anything"));
        assert!(wildcard("npm run *", "npm run build"));
        assert!(!wildcard("npm run *", "npm test"));
    }

    #[test]
    fn the_gate_takes_flags_after_the_files_and_the_flags_mode_first() {
        let base = std::env::temp_dir().join(format!(
            "glut-gate-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let (home, project) = (base.join("home"), base.join("project"));
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            home.join(".glut/settings.json"),
            r#"{"permissions": {"defaultMode": "plan"}}"#,
        )
        .unwrap();
        let options = GateOptions {
            mode: None,
            allow: vec!["Bash(make:*)".into()],
            deny: vec!["Edit(secrets/**)".into()],
            ..GateOptions::default()
        };
        let built = build_gate(&home, &project, options.clone(), Arc::new(Nobody)).unwrap();
        assert!(built.sandbox.enabled, "on unless the owner switches it off");
        assert_eq!(built.gate.mode(), Mode::Plan);
        assert!(!built.untrusted_project);
        let flagged = GateOptions {
            mode: Some(Mode::Auto),
            ..options
        };
        assert_eq!(
            build_gate(&home, &project, flagged, Arc::new(Nobody))
                .unwrap()
                .gate
                .mode(),
            Mode::Auto
        );
        let broken = GateOptions {
            allow: vec!["Bash(".into()],
            ..GateOptions::default()
        };
        assert!(build_gate(&home, &project, broken, Arc::new(Nobody)).is_err());
    }

    #[test]
    fn the_notes_of_a_built_gate_say_what_was_left_out_and_a_sandbox_that_is_off() {
        let built = |untrusted_project, project_hooks_skipped, enabled| BuiltGate {
            gate: Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody)),
            untrusted_project,
            sandbox: settings::SandboxSettings {
                enabled,
                ..settings::SandboxSettings::default()
            },
            hooks: crate::hooks::Hooks::default(),
            project_hooks_skipped,
            effort: None,
            refusals: crate::config::RefusalLimits::DEFAULT,
            compact_auto: true,
            context_windows: std::collections::HashMap::new(),
            prices: std::collections::HashMap::new(),
            parallel_subagents: crate::config::DEFAULT_PARALLEL_SUBAGENTS,
            guide_warn_bytes: u64::from(crate::config::DEFAULT_GUIDE_WARN_KB) * 1024,
            notify: settings::NotifySection::default(),
            loops: settings::LoopSection::default(),
            nudge: true,
            lsp: true,
            max_turns: None,
        };
        let off = built(true, true, false).notes();
        assert_eq!(off.len(), 3, "{off:?}");
        assert!(off[0].contains("not trusted"));
        assert!(off[1].contains("hooks are not run"));
        assert!(off[2].contains("without a sandbox"));
        // Nothing left out and the sandbox on: at most the note that this system has none.
        let on = built(false, false, true).notes();
        assert_eq!(on.is_empty(), crate::sandbox::available(), "{on:?}");
    }

    #[test]
    fn typos_in_rule_names_are_refused_rather_than_silently_ignored() {
        for bad in ["bash(rm:*)", "BASH(rm:*)", "read(.env)", "edit"] {
            assert!(Rule::parse(bad).is_err(), "{bad:?}");
        }
        assert!(
            Rule::parse("grep").is_ok(),
            "a tool's own name stays allowed"
        );
    }

    #[test]
    fn deny_rules_hold_through_spacing_wrappers_paths_and_shells() {
        let set = rules(&["Bash"], &[], &["Bash(rm:*)", "Bash(git push:*)"]);
        for command in [
            "rm\t-rf x",
            "git  push origin",
            "FOO=1 rm x",
            "env rm x",
            "sudo rm x",
            "/bin/rm x",
            "command rm x",
            "xargs rm",
            "nohup rm x",
            "r\"\"m x",
            "\\rm x",
            "bash -c 'rm x'",
            "true && rm x",
        ] {
            assert_eq!(
                said(&set, "bash", bash(command)),
                Some(Verdict::Deny),
                "{command:?}"
            );
        }
    }

    #[test]
    fn a_command_the_lexer_cannot_vouch_for_is_never_allowed_even_in_auto_mode() {
        let set = rules(&["Bash(echo:*)", "Bash(cat:*)"], &[], &[]);
        for command in [
            "echo ok #'\nrm -rf x",
            "cat <<EOF\nit's\nEOF\nrm x",
            "echo $'\\''; rm x",
            "(rm x)",
        ] {
            assert_eq!(
                said(&set, "bash", bash(command)),
                Some(Verdict::Ask),
                "{command:?}"
            );
        }
    }

    #[test]
    fn what_a_command_writes_is_held_against_protection_and_edit_rules() {
        let set = rules(
            &["Bash(echo:*)", "Edit(notes/**)"],
            &[],
            &["Edit(secrets/**)"],
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > .glut/settings.json")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(&set, "bash", bash("echo x >> .GIT/config")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > secrets/key")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > ~/.zshrc")),
            Some(Verdict::Ask),
            "outside the folder"
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > notes/a.md")),
            Some(Verdict::Allow),
            "an edit rule allows it"
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > other.md")),
            None,
            "no edit rule: the mode decides"
        );
        assert_eq!(
            said(&set, "bash", bash("echo x 2>&1")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > /dev/null")),
            Some(Verdict::Allow)
        );
    }

    #[test]
    fn assignments_and_protected_names_keep_a_command_from_being_allowed_by_rule() {
        let set = rules(&["Bash(cargo test:*)", "Bash(cat:*)"], &[], &[]);
        assert_eq!(
            said(&set, "bash", bash("DYLD_INSERT_LIBRARIES=x cargo test")),
            None
        );
        assert_eq!(
            said(&set, "bash", bash("cat .git/config")),
            Some(Verdict::Ask)
        );
        assert_eq!(
            said(&set, "bash", bash("cargo test 2>&1 | cat")),
            Some(Verdict::Allow)
        );
    }

    #[test]
    fn path_rules_resolve_dots_and_keep_star_within_a_folder() {
        let set = rules(&["Edit(src/*.rs)"], &[], &["Read(.env)"]);
        let edit = |path: &str| Access::Edit { path: path.into() };
        assert_eq!(
            said(&set, "write_file", edit("src/a.rs")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&set, "write_file", edit("src/deep/a.rs")),
            None,
            "* does not cross a folder"
        );
        assert_eq!(
            said(
                &set,
                "read_file",
                Access::Read {
                    path: "src/../.env".into()
                }
            ),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(
                &set,
                "read_file",
                Access::Read {
                    path: "a//../.env".into()
                }
            ),
            Some(Verdict::Deny)
        );
        assert_eq!(normal_path("../x"), "../x");
        assert_eq!(normal_path("./"), ".");
    }

    #[tokio::test]
    async fn modes_that_ask_nothing_refuse_asks_and_plan_writes_nothing() {
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let dont_ask = Gate::new(
            rules(&[], &["Bash(make:*)"], &[]),
            Mode::DontAsk,
            asker.clone(),
        );
        assert!(dont_ask.judge("bash", &bash("make")).await.is_err());
        let plan = Gate::new(rules(&["Edit"], &[], &[]), Mode::Plan, asker.clone());
        assert!(
            plan.judge("write_file", &Access::Edit { path: "a".into() })
                .await
                .is_err()
        );
        let auto = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(
            auto.judge("bash", &bash("echo $(whoami)")).await.is_ok(),
            "auto trusts what the sandbox holds"
        );
        assert!(
            auto.judge("bash", &bash("sudo make")).await.is_ok(),
            "asked, and the asker said yes"
        );
        assert_eq!(
            *asker.1.lock().unwrap(),
            1,
            "only the heavy command in auto mode was asked"
        );
    }

    #[test]
    fn invisible_characters_never_reach_the_terminal() {
        assert_eq!(printable("rm\u{202E}fdp.exe\u{1b}[2J"), "rm fdp.exe [2J");
        assert_eq!(printable("a\u{200B}b"), "a b");
    }

    #[test]
    fn the_scopes_offered_go_from_the_command_to_its_program_for_every_part_at_once() {
        let labels = |command: &str| {
            scopes("bash", &bash(command), false)
                .into_iter()
                .map(|scope| scope.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels("git status --short"),
            ["git status --short", "git status *", "git *"]
        );
        assert_eq!(
            labels("cargo fmt && cargo test -p x"),
            [
                "cargo fmt, cargo test -p x",
                "cargo fmt *, cargo test *",
                "cargo *"
            ]
        );
        assert_eq!(labels("make"), ["make"]);
        assert_eq!(
            labels("cargo --version | head -1"),
            ["cargo --version", "cargo *"],
            "head only reads: no rule for it"
        );
        assert!(labels("FOO=1 make").is_empty());
        assert!(labels("echo $(x)").is_empty());
        assert!(
            labels("sudo make").is_empty(),
            "a wrapper would reach every program after it"
        );
    }

    #[tokio::test]
    async fn the_owner_picks_how_far_an_allowance_reaches() {
        let asker = Arc::new(Always(Answer::AllowWithin(1), Mutex::new(0)));
        let gate = Gate::new(RuleSet::default(), Mode::Default, asker.clone());
        assert!(gate.judge("bash", &bash("git commit -m one")).await.is_ok());
        assert!(gate.judge("bash", &bash("git commit -m two")).await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 1, "git commit * was allowed");
        assert!(
            gate.judge("bash", &bash("git commit -m three | tail -2"))
                .await
                .is_ok(),
            "a reading part needs no rule of its own"
        );
        assert_eq!(*asker.1.lock().unwrap(), 1);
        let asker = Arc::new(Always(Answer::Deny, Mutex::new(0)));
        let rules = gate.rules();
        let after = Gate::new(rules, Mode::Default, asker.clone());
        assert!(
            after.judge("bash", &bash("git push")).await.is_err(),
            "but not git *"
        );
        let gate = Gate::new(
            RuleSet::default(),
            Mode::Default,
            Arc::new(Always(Answer::AllowWithin(9), Mutex::new(0))),
        );
        assert!(gate.judge("bash", &bash("git commit -m one")).await.is_ok());
        assert!(
            gate.take_log()[0]
                .reason
                .ends_with("from now on: git commit -m one"),
            "an index past the last scope takes the narrowest"
        );
    }

    /// T61: an arena's model is never asked — what would be asked is refused at once, the rest runs as in auto.
    #[tokio::test]
    async fn an_arena_gate_asks_nothing_and_refuses_what_it_would_ask() {
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let gate = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        gate.set_contained(false);
        assert!(!gate.can_ask());
        for runs in ["cargo test", "rm -f a.txt", "git status && ls"] {
            let judged = gate.judge("bash", &bash(runs)).await;
            assert!(judged.is_ok(), "{runs}: {judged:?}");
        }
        for refused in [
            "rm -rf build",
            "git push origin main",
            "git reset --hard HEAD",
            "for f in a; do rm \"$f\"; done",
        ] {
            let judged = gate.judge("bash", &bash(refused)).await.unwrap_err();
            assert!(
                judged.contains("an arena's model asks no one"),
                "{refused}: {judged}"
            );
        }
        assert_eq!(gate.ask_owner("exit_plan", "plan").await, Answer::Deny);
        assert_eq!(*asker.1.lock().unwrap(), 0, "no one was asked");
        let open = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(open.judge("bash", &bash("rm -rf build")).await.is_ok());
        assert_eq!(*asker.1.lock().unwrap(), 1, "another gate still asks");
    }

    /// T61: a confined arena's model runs a heavy command its own folder holds, never one that reaches past it, and
    /// reaches the package registries alone.
    #[tokio::test]
    async fn a_confined_arena_gate_runs_heavy_commands_that_reach_no_further() {
        let asker = Arc::new(Always(Answer::Allow, Mutex::new(0)));
        let gate = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        gate.set_contained(true);
        for runs in [
            "rm -rf build",
            "git reset --hard HEAD~2",
            "git branch -D old && git clean -fdx",
            "for f in a b; do rm -rf \"$f\"; done",
            "find . -name '*.pyc' -delete",
        ] {
            let judged = gate.judge("bash", &bash(runs)).await;
            assert!(judged.is_ok(), "{runs}: {judged:?}");
        }
        for refused in [
            "sudo rm -rf build",
            "git push origin main",
            "rm -rf x && git push",
            "npm publish",
            "gh release create v1",
            "killall node",
            "eval \"git push\"",
            "for h in a; do sudo \"$h\"; done",
            "rm -rf build && cp x .cargo/config.toml",
        ] {
            let judged = gate.judge("bash", &bash(refused)).await;
            assert!(judged.is_err(), "{refused}: {judged:?}");
        }
        let net = |host: &str| Access::Network { host: host.into() };
        assert!(
            gate.judge("network", &net("index.crates.io:443"))
                .await
                .is_ok()
        );
        assert!(gate.judge("network", &net("pypi.org")).await.is_ok());
        assert!(
            gate.judge("network", &net("pypi.org:8080")).await.is_err(),
            "the web's ports alone"
        );
        let elsewhere = gate
            .judge("network", &net("example.com:443"))
            .await
            .unwrap_err();
        assert!(
            elsewhere.contains("only the package registries"),
            "{elsewhere}"
        );
        assert_eq!(*asker.1.lock().unwrap(), 0, "no one was asked");
        // Contained without its folder confined: a heavy command stays refused.
        let unconfined = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        unconfined.set_contained(false);
        assert!(
            unconfined
                .judge("bash", &bash("rm -rf build"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reading_commands_run_in_every_mode_and_auto_asks_only_heavy_ones() {
        let asker = Arc::new(Always(Answer::Deny, Mutex::new(0)));
        for mode in [Mode::Plan, Mode::Default, Mode::AcceptEdits, Mode::Auto] {
            let gate = Gate::new(rules(&[], &[], &["Read(.env)"]), mode, asker.clone());
            assert!(
                gate.judge(
                    "bash",
                    &bash("grep -rn x src | head -20; ls -la docs 2>/dev/null")
                )
                .await
                .is_ok(),
                "{mode:?}"
            );
            for held in [
                "cat .env",
                "grep x < .env",
                "grep --file=.env x",
                "git show HEAD:.env",
            ] {
                assert!(
                    gate.judge("bash", &bash(held)).await.is_err(),
                    "{mode:?}: a rule holds {held} back"
                );
            }
            let outside = gate.judge("bash", &bash("cat ~/.zshenv")).await;
            assert_eq!(
                outside.is_ok(),
                mode == Mode::Auto,
                "{mode:?}: outside the folder only auto reads freely"
            );
        }
        let dont_ask = Gate::new(RuleSet::default(), Mode::DontAsk, asker.clone());
        assert!(
            dont_ask.judge("bash", &bash("ls")).await.is_err(),
            "dont_ask keeps to its rules alone"
        );
        let unfenced = |command: &str| {
            let mut access = bash(command);
            if let Access::Bash { sandboxed, .. } = &mut access {
                *sandboxed = false;
            }
            access
        };
        let before = *asker.1.lock().unwrap();
        let auto = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(
            auto.judge("bash", &unfenced("echo $(whoami)"))
                .await
                .is_err(),
            "without a sandbox auto asks what the rules cannot read, as before"
        );
        assert!(
            auto.judge("bash", &unfenced("rm -rf target"))
                .await
                .is_err(),
            "without a sandbox auto asks a command the rules read too"
        );
        assert_eq!(*asker.1.lock().unwrap(), before + 2);
        let build = Gate::new(RuleSet::default(), Mode::AcceptEdits, asker.clone());
        assert!(
            build.judge("bash", &unfenced("ls")).await.is_err(),
            "without a sandbox a reader is asked"
        );
        let asked = *asker.1.lock().unwrap();
        let auto = Gate::new(RuleSet::default(), Mode::Auto, asker.clone());
        assert!(
            auto.judge("bash", &bash("cargo build && echo $HOME > /tmp/x"))
                .await
                .is_ok()
        );
        assert!(
            auto.judge("bash", &bash("rm -rf target")).await.is_err(),
            "asked and refused"
        );
        assert!(auto.judge("bash", &bash("git push")).await.is_err());
        assert!(
            auto.judge("bash", &bash("cp x sub/.cargo/config.toml"))
                .await
                .is_err(),
            "a protected name is asked even in auto"
        );
        assert!(
            auto.judge("bash", &bash("$(echo x) rm -rf y"))
                .await
                .is_err(),
            "a hidden rm is found"
        );
        assert_eq!(
            *asker.1.lock().unwrap(),
            asked + 4,
            "only the heavy and protected ones were asked"
        );
        let reason = &auto.take_log()[1].reason;
        assert!(reason.contains("asked even in auto mode"), "{reason}");
    }

    /// A tool standing in a folder, as the shell does.
    struct Standing(std::path::PathBuf);

    impl crate::tools::Tool for Standing {
        fn spec(&self) -> crate::ToolSpec {
            crate::ToolSpec {
                name: "bash".into(),
                description: String::new(),
                parameters: serde_json::json!({}),
            }
        }
        fn access(&self, _arguments: &serde_json::Value) -> Access {
            bash("")
        }
        fn call(
            &self,
            _arguments: serde_json::Value,
            _context: crate::tools::CallContext,
        ) -> crate::ToolFuture<'_> {
            Box::pin(async { Ok(String::new()) })
        }
        fn here(&self) -> Option<std::path::PathBuf> {
            Some(self.0.clone())
        }
    }

    #[tokio::test]
    async fn auto_runs_a_glob_where_it_finds_no_protected_file_and_asks_where_it_does() {
        let dir = std::env::temp_dir().join(format!("glut-gate-glob-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let shell: Arc<dyn crate::tools::Tool> = Arc::new(Standing(dir.clone()));
        let auto = Gate::new(RuleSet::default(), Mode::Auto, Arc::new(Nobody));
        assert!(
            auto.judge("bash", &bash("wc -l data/*")).await.is_err(),
            "no shell tied: the glob counts"
        );
        auto.set_here(Arc::downgrade(&shell));
        assert!(auto.judge("bash", &bash("wc -l data/*")).await.is_ok());
        std::fs::write(dir.join("data/trusted.json"), "").unwrap();
        assert!(auto.judge("bash", &bash("wc -l data/*")).await.is_err());
        assert!(
            auto.judge("bash", &bash("ls .gi*")).await.is_err(),
            "a dotted name behind a dot, wherever"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_lands_where_the_shell_is_and_cd_with_a_write_is_asked() {
        let set = rules(
            &["Bash(echo:*)", "Bash(cd:*)", "Edit(**)"],
            &[],
            &["Edit(secrets/**)"],
        );
        let inside = Access::Bash {
            command: "echo x > f.txt".into(),
            folder: "secrets".into(),
            unsandboxed: false,
            sandboxed: false,
        };
        assert_eq!(said(&set, "bash", inside), Some(Verdict::Deny));
        assert_eq!(
            said(&set, "bash", bash("cd secrets && echo x > f.txt")),
            Some(Verdict::Ask)
        );
        assert_eq!(
            said(&set, "bash", bash("echo x > f.txt")),
            Some(Verdict::Allow)
        );
    }

    #[test]
    fn a_trailing_star_also_means_no_arguments_and_command_names_follow_the_file_system() {
        let set = rules(&["Bash"], &[], &["Bash(git push *)", "Bash(rm:*)"]);
        assert_eq!(said(&set, "bash", bash("git push")), Some(Verdict::Deny));
        assert_eq!(
            said(&set, "bash", bash("git push origin")),
            Some(Verdict::Deny)
        );
        let upper = if command::CASE_INSENSITIVE_COMMANDS {
            Some(Verdict::Deny)
        } else {
            Some(Verdict::Allow)
        };
        assert_eq!(said(&set, "bash", bash("RM -rf x")), upper);
        assert_eq!(
            said(&set, "bash", bash("git -C . push")),
            Some(Verdict::Deny)
        );
    }

    #[test]
    fn a_folder_rule_covers_the_folder_itself() {
        let set = rules(&[], &[], &["Read(secrets/**)"]);
        assert_eq!(
            said(
                &set,
                "list_dir",
                Access::Read {
                    path: "secrets".into()
                }
            ),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(
                &set,
                "list_dir",
                Access::Read {
                    path: "secret".into()
                }
            ),
            None
        );
    }

    #[tokio::test]
    async fn plan_mode_refuses_a_command_that_writes() {
        let plan = Gate::new(
            rules(&["Bash(echo:*)", "Edit(**)"], &[], &[]),
            Mode::Plan,
            Arc::new(Nobody),
        );
        assert!(plan.judge("bash", &bash("echo x > a.txt")).await.is_err());
        assert!(plan.judge("bash", &bash("echo x")).await.is_ok());
    }

    #[test]
    fn an_ask_shows_line_breaks_and_both_ends_of_a_long_command() {
        assert_eq!(ask_display("echo hi\nrm -rf ~"), "echo hi ⏎ rm -rf ~");
        let long = format!("echo {}; rm -rf ~", "a".repeat(400));
        let shown = ask_display(&long);
        assert!(
            shown.ends_with("rm -rf ~") && shown.contains("characters left out"),
            "{shown}"
        );
    }

    #[test]
    fn a_command_outside_the_sandbox_is_always_asked_unless_denied() {
        let set = rules(&["Bash(npm install:*)"], &[], &["Bash(rm:*)"]);
        let out = |command: &str| Access::Bash {
            command: command.into(),
            folder: String::new(),
            unsandboxed: true,
            sandboxed: false,
        };
        assert_eq!(
            said(&set, "bash", out("npm install")),
            Some(Verdict::Ask),
            "the allow rule does not carry over"
        );
        assert_eq!(said(&set, "bash", out("rm x")), Some(Verdict::Deny));
    }

    /// T60c: a command writing into a folder beside the working one is judged as a write there — by the edit rules on
    /// its path as shown —, and one that only reads there reads as in the working folder; without the folder both
    /// stay "outside".
    #[tokio::test]
    async fn a_command_in_a_folder_beside_is_judged_as_in_the_working_one() {
        let gate = Gate::new(
            rules(
                &["Bash(echo:*)", "Edit(~/Vault/**)"],
                &[],
                &["Edit(~/Vault/locked/**)"],
            ),
            Mode::Default,
            Arc::new(Nobody),
        );
        let write = bash("echo x > /Users/o/Vault/a.md");
        assert!(
            gate.judge("bash", &write).await.is_err(),
            "outside: asked, nobody answers"
        );
        let folders = Folders::default();
        folders
            .add(crate::folders::Folder {
                real: "/Users/o/Vault".into(),
                shown: "~/Vault".into(),
                git_dirs: Vec::new(),
            })
            .unwrap();
        gate.set_beside(folders);
        assert!(
            gate.judge("bash", &write).await.is_ok(),
            "allowed by Edit(~/Vault/**)"
        );
        assert!(
            gate.judge("bash", &bash("echo x > ~/Vault/b.md"))
                .await
                .is_ok()
        );
        let locked = gate.judge("bash", &bash("echo x > ~/Vault/locked/c")).await;
        assert!(locked.is_err_and(|why| why.contains("Vault/locked")));
        for out in [
            "echo x > ~/Vault/../.zshrc",
            "echo x > $HOME/Vault/a",
            "echo x > ~/Vaults/a",
        ] {
            assert!(gate.judge("bash", &bash(out)).await.is_err(), "{out}");
        }
        assert!(
            gate.judge("bash", &bash("cat /Users/o/Vault/a.md"))
                .await
                .is_ok()
        );
        let held = Gate::new(
            rules(&[], &[], &["Read(~/Vault/secret/**)"]),
            Mode::Default,
            Arc::new(Nobody),
        );
        held.set_beside(gate.beside());
        assert!(
            held.judge("bash", &bash("cat ~/Vault/secret/x"))
                .await
                .is_err()
        );
        assert!(
            held.judge("bash", &bash("cat ~/Vault/open.md"))
                .await
                .is_ok()
        );
        assert!(
            gate.judge("bash", &bash("cat ~/elsewhere.md"))
                .await
                .is_err()
        );
    }

    #[test]
    fn network_rules_name_a_host_or_everything_below_a_domain() {
        let set = rules(
            &["Network(crates.io)", "Network(*.github.com)"],
            &[],
            &["Network(evil.github.com)"],
        );
        let host = |name: &str| Access::Network { host: name.into() };
        assert_eq!(
            said(&set, "network", host("crates.io")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&set, "network", host("static.crates.io")),
            None,
            "a name is only itself"
        );
        assert_eq!(
            said(&set, "network", host("api.github.com")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&set, "network", host("github.com")),
            None,
            "*. means below, not the name itself"
        );
        assert_eq!(
            said(&set, "network", host("evil.github.com")),
            Some(Verdict::Deny)
        );
        assert_eq!(said(&set, "network", host("notgithub.com")), None);
        // Claude Code's spelling speaks about `web_fetch` alone — never about the shell's proxy (glut-round CP2).
        let web = Rule::parse("WebFetch(domain:docs.rs)").unwrap();
        assert!(web.matches_path("web_fetch", &host("docs.rs")));
        assert!(!web.matches_path("web_fetch", &host("evil.rs")));
        assert!(
            Rule::parse("WebFetch")
                .unwrap()
                .matches_path("web_fetch", &host("any.org"))
        );
        assert!(
            !Rule::parse("WebFetch")
                .unwrap()
                .matches_path(crate::sandbox::proxy::NETWORK_TOOL, &host("any.org"))
        );
        // `Network(…)` keeps serving both.
        assert!(
            Rule::parse("Network(docs.rs)")
                .unwrap()
                .matches_path("web_fetch", &host("docs.rs"))
        );
        assert!(Rule::parse("webfetch(domain:x.org)").is_err());
        // `WebSearch` names the `web_search` tool.
        assert!(Rule::parse("WebSearch").unwrap().names_tool("web_search"));
        assert!(Rule::parse("websearch").is_err());
        for bad in ["Network(http://x)", "Network(a b)", "network(x)"] {
            assert!(Rule::parse(bad).is_err(), "{bad}");
        }
        let always = always_rule("network", &host("pypi.org")).unwrap();
        assert!(always.matches_path("network", &host("pypi.org")));
    }

    /// glut-round CP4: a run is offered only the tools its declaration could name — patterns left aside — no tool a
    /// pattern-less deny refuses whole, and no edit tool in Plan mode; a run nothing narrows is offered everything.
    #[test]
    fn a_run_is_offered_only_the_tools_its_declaration_could_name() {
        let gate = |mode: Mode, declared: &[&str], deny: &[&str]| {
            let mut gate = Gate::new(rules(&[], &[], deny), mode, Arc::new(Nobody));
            gate.narrow(
                "the skill news",
                declared
                    .iter()
                    .map(|text| Rule::parse(text).unwrap())
                    .collect(),
            );
            gate
        };
        let read = Access::Read {
            path: String::new(),
        };
        let edit = Access::Edit {
            path: String::new(),
        };
        let shell = bash("");
        let page = Access::Network {
            host: String::new(),
        };
        let mail = |tool: &str| Access::Mcp {
            server: "apple-mail".into(),
            tool: tool.into(),
        };

        let news = gate(
            Mode::Default,
            &[
                "Read",
                "WebFetch",
                "Edit(News/**)",
                "Bash(grep:*)",
                "mcp__apple-mail__search_emails",
            ],
            &[],
        );
        assert!(news.offers("read_file", &read));
        assert!(
            news.offers("edit_file", &edit),
            "a pattern still names the tool"
        );
        assert!(news.offers("bash", &shell));
        assert!(news.offers("bash_output", &shell));
        assert!(news.offers("web_fetch", &page));
        assert!(news.offers("mcp__apple-mail__search_emails", &mail("search_emails")));
        assert!(!news.offers("mcp__apple-mail__send_email", &mail("send_email")));
        assert!(!news.offers("web_search", &Access::Other));
        assert!(!news.offers("skill", &Access::Other));
        assert!(
            news.offers("todo", &Access::Own),
            "the run's own state is always offered"
        );

        let reader = gate(Mode::Default, &["Read"], &[]);
        for (tool, access) in [
            ("write_file", &edit),
            ("bash", &shell),
            ("web_fetch", &page),
        ] {
            assert!(!reader.offers(tool, access), "{tool}");
        }
        // `Network(…)` serves the shell's proxy and the fetch alike; a whole MCP server names each of its tools.
        let wide = gate(
            Mode::Default,
            &["Network(docs.rs)", "mcp__apple-mail", "WebSearch"],
            &[],
        );
        assert!(wide.offers("web_fetch", &page));
        assert!(wide.offers("mcp__apple-mail__send_email", &mail("send_email")));
        assert!(wide.offers("web_search", &Access::Other));

        let open = gate(Mode::Default, &[], &[]);
        for (tool, access) in [
            ("write_file", &edit),
            ("bash", &shell),
            ("skill", &Access::Other),
        ] {
            assert!(open.offers(tool, access), "{tool}");
        }
        let plan = gate(Mode::Plan, &[], &[]);
        assert!(!plan.offers("write_file", &edit));
        assert!(plan.offers("read_file", &read));
        // A deny without a pattern takes the tool away; one with a pattern leaves it for the other calls.
        let denied = gate(Mode::Default, &[], &["WebFetch", "Bash(rm:*)"]);
        assert!(!denied.offers("web_fetch", &page));
        assert!(denied.offers("bash", &shell));
    }

    /// A command's `allowed-tools` narrows the gate (HS4, C3): what it does not name is refused before any rule or
    /// mode speaks, what it names is judged as usual — and such a rule never allows anything by itself.
    #[tokio::test]
    async fn a_commands_allowed_tools_only_narrows_and_never_allows() {
        let narrowed = |mode: Mode, restrict: &[&str]| {
            let mut gate = Gate::new(RuleSet::default(), mode, Arc::new(Nobody));
            gate.narrow(
                "the command /review",
                restrict
                    .iter()
                    .map(|text| Rule::parse(text).unwrap())
                    .collect(),
            );
            gate
        };
        let read = Access::Read {
            path: "src/lib.rs".into(),
        };
        let edit = Access::Edit {
            path: "src/lib.rs".into(),
        };
        let git_status = bash("git status");

        let only_read = narrowed(Mode::Default, &["Read"]);
        assert!(
            only_read.judge("read_file", &read).await.is_ok(),
            "a call the command allows is judged as usual"
        );
        for (tool, access) in [("bash", &git_status), ("write_file", &edit)] {
            let refused = only_read.judge(tool, access).await.unwrap_err();
            assert!(
                refused.contains("the command /review allows only: Read"),
                "{refused}"
            );
        }
        assert!(
            only_read.judge("bash_output", &Access::Own).await.is_ok(),
            "the run's own state stays always allowed"
        );

        // A command rule in `allowed-tools` does not free a command from the mode (C3).
        let plan = narrowed(Mode::Plan, &["Bash(cargo test:*)"]);
        let refused = plan.judge("bash", &bash("cargo test")).await.unwrap_err();
        assert!(refused.contains("mode plan"), "{refused}");
    }

    /// A skill run started with a slash command (`glut run --skill s "/cmd"`): both `allowed-tools` lists narrow
    /// the run, so the second can only take away — a call needs both, and the refusal names the one that left
    /// it out.
    #[tokio::test]
    async fn a_second_narrowing_never_widens_the_first() {
        let parse = |list: &[&str]| list.iter().map(|text| Rule::parse(text).unwrap()).collect();
        let mut gate = Gate::new(
            rules(&["Bash(git status:*)"], &[], &[]),
            Mode::Default,
            Arc::new(Nobody),
        );
        gate.narrow("the command /look", parse(&["Read"]));
        gate.narrow("the skill git", parse(&["Read", "Bash(git status:*)"]));
        assert_eq!(gate.narrowed_by(), ["the command /look", "the skill git"]);

        let read = Access::Read {
            path: "src/lib.rs".into(),
        };
        assert!(gate.judge("read_file", &read).await.is_ok());
        let refused = gate.judge("bash", &bash("git status")).await.unwrap_err();
        assert!(
            refused.contains("the command /look allows only: Read"),
            "{refused}"
        );

        // An empty list declares nothing and narrows nothing.
        let mut open = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        open.narrow("the skill quiet", Vec::new());
        assert!(open.narrowed_by().is_empty());
    }

    #[tokio::test]
    async fn an_agents_tools_narrow_its_own_calls_only() {
        let parse = |list: &[&str]| list.iter().map(|text| Rule::parse(text).unwrap()).collect();
        let gate = Gate::new(
            rules(&["Bash(git status:*)"], &[], &[]),
            Mode::Default,
            Arc::new(Nobody),
        );
        let read = Access::Read {
            path: "src/lib.rs".into(),
        };
        let (read_ok, refused, offered_bash) = as_agent(
            "c1".into(),
            "look".into(),
            Some(("the agent reader".into(), parse(&["Read"]))),
            async {
                (
                    gate.judge("read_file", &read).await.is_ok(),
                    gate.judge("bash", &bash("git status")).await.unwrap_err(),
                    gate.offers("bash", &bash("git status")),
                )
            },
        )
        .await;
        assert!(read_ok);
        assert!(
            refused.contains("the agent reader allows only: Read"),
            "{refused}"
        );
        assert!(
            !offered_bash,
            "a tool its agent never names is not offered to it"
        );
        // Its commands reach no network unless its tools name some; the run's always may.
        let offline = as_agent(
            "c3".into(),
            "x".into(),
            Some(("a".into(), parse(&["Bash"]))),
            async { agent_reaches_network() },
        )
        .await;
        let online = as_agent(
            "c4".into(),
            "x".into(),
            Some(("a".into(), parse(&["Bash", "Network(docs.rs)"]))),
            async { agent_reaches_network() },
        )
        .await;
        assert!(!offline && online && agent_reaches_network());
        // An empty list allows nothing at all.
        let nothing = as_agent(
            "c5".into(),
            "x".into(),
            Some(("the agent mute".into(), Vec::new())),
            async { gate.judge("read_file", &read).await },
        )
        .await;
        assert!(nothing.is_err());
        // The run itself, and a subagent without an agent, keep the rules as they are.
        assert!(gate.judge("bash", &bash("git status")).await.is_ok());
        let plain = as_subagent(
            "c2".into(),
            "plain".into(),
            gate.judge("bash", &bash("git status")),
        )
        .await;
        assert!(plain.is_ok());
    }

    /// An MCP call as the model makes it (M5): the server's own names.
    fn mcp(server: &str, tool: &str) -> Access {
        Access::Mcp {
            server: server.into(),
            tool: tool.into(),
        }
    }

    #[test]
    fn an_mcp_rule_names_a_server_or_one_of_its_tools() {
        // `mcp__mail` speaks about every tool of `mail`, `mcp__mail__send` about that one (M5).
        let every = rules(&["mcp__mail"], &[], &[]);
        assert_eq!(
            said(&every, "mcp__mail__send", mcp("mail", "send")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&every, "mcp__mail__list", mcp("mail", "list")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&every, "mcp__notes__list", mcp("notes", "list")),
            None,
            "another server is not spoken about"
        );

        let one = rules(&["mcp__mail__send"], &[], &[]);
        assert_eq!(
            said(&one, "mcp__mail__send", mcp("mail", "send")),
            Some(Verdict::Allow)
        );
        assert_eq!(
            said(&one, "mcp__mail__list", mcp("mail", "list")),
            None,
            "another tool of the same server is not named"
        );

        // deny beats allow, across the two spellings (M5).
        let both = rules(&["mcp__mail"], &[], &["mcp__mail__send"]);
        assert_eq!(
            said(&both, "mcp__mail__send", mcp("mail", "send")),
            Some(Verdict::Deny)
        );
        assert_eq!(
            said(&both, "mcp__mail__list", mcp("mail", "list")),
            Some(Verdict::Allow)
        );

        // A server whose own name holds `__` is still read right: the rule is the whole rest after `mcp__`, or
        // the server up to the next `__`.
        let nested = rules(&["mcp__a__b"], &[], &[]);
        assert_eq!(
            said(&nested, "mcp__a__b", mcp("a__b", "c")),
            Some(Verdict::Allow),
            "the whole rest after mcp__ is the server"
        );
        assert_eq!(
            said(&nested, "mcp__a__b__c", mcp("a", "b")),
            Some(Verdict::Allow),
            "or the server up to the next __"
        );
        assert_eq!(
            said(&nested, "mcp__a__b__c", mcp("a__b", "c")),
            Some(Verdict::Allow)
        );

        // A pattern has nothing to add to an MCP rule: `mcp__mail(send)` would be a rule about a tool called
        // `mail(send)` that never matches — a deny that silently does nothing (M5).
        for bad in ["mcp__mail(send)", "mcp__mail(*)", "mcp__"] {
            assert!(Rule::parse(bad).is_err(), "{bad}");
        }
        assert!(Rule::parse("mcp__mail__send").is_ok());
    }

    #[tokio::test]
    async fn without_a_rule_the_mode_decides_an_mcp_call() {
        let access = mcp("mail", "send");
        let judge = |mode: Mode| Gate::new(RuleSet::default(), mode, Arc::new(Nobody));
        // `default` and `accept_edits` ask, and nobody to ask means refused — never run (M5).
        for mode in [Mode::Default, Mode::AcceptEdits] {
            let refused = judge(mode).judge("mcp__mail__send", &access).await;
            assert!(refused.is_err(), "{mode:?} must not run it: {refused:?}");
        }
        // `auto` allows, `plan` and `dont_ask` refuse (M5).
        assert!(
            judge(Mode::Auto)
                .judge("mcp__mail__send", &access)
                .await
                .is_ok()
        );
        for mode in [Mode::Plan, Mode::DontAsk] {
            let refused = judge(mode).judge("mcp__mail__send", &access).await;
            assert!(refused.is_err(), "{mode:?} must refuse it: {refused:?}");
        }
        // A rule still speaks first: `auto` does not save a denied tool.
        let denied = Gate::new(
            rules(&[], &[], &["mcp__mail__send"]),
            Mode::Auto,
            Arc::new(Nobody),
        );
        assert!(denied.judge("mcp__mail__send", &access).await.is_err());
    }
}
