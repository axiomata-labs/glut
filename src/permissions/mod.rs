//! Who may do what in a run (`docs/plans/harness.md`, HS3a, P1–P8): rules in Claude Code's format, modes, and asking
//! the owner.
//!
//! Every tool says what a call would touch ([`Access`]); the [`Gate`] judges it against the rules — `deny` beats
//! `ask` beats `allow`, across every source — and, where no rule speaks, against the run's [`Mode`]. An `ask` goes to
//! the run's [`Asker`]: the terminal, the app, or nobody (refused). Rules are patterns, not a fence: the sandbox of
//! HS3b is what stops a command that slips past them.

pub mod command;
pub mod settings;

use std::fmt;
use std::future::Future;
use std::path::{Component, Path};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;

use command::{Analysis, analyze, names_protected};

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
    },
    /// A sandboxed command reaching a host through the run's proxy (HS3b): the host name, lowercased.
    Network { host: String },
    /// An MCP tool call (HS5): the server's name and the tool's own name on it. The model sees it as
    /// `mcp__<server>__<tool>`, which is what the rules are written as.
    Mcp { server: String, tool: String },
    /// Only the run's own state (`bash_output`, `kill_shell`): always allowed.
    Own,
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
        })
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
            Access::Other => named,
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
    /// Reads free; edits and commands asked.
    #[default]
    Default,
    /// Reads and edits free; commands asked.
    AcceptEdits,
    /// Reads only: edits refused even where a rule allows them, commands refused unless a rule allows them, nothing
    /// asked.
    Plan,
    /// Whatever no rule allows is refused; nothing is asked.
    DontAsk,
    /// Whatever no rule denies is allowed (the owner's auto mode) — except a command the rules cannot read, which is
    /// asked.
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
            "accept_edits" | "acceptEdits" => Some(Mode::AcceptEdits),
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
            (Mode::Auto, _) => Allow,
            (_, Access::Read { .. }) => Allow,
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
    /// Adds `other`'s rules to these.
    pub fn extend(&mut self, other: RuleSet) {
        self.allow.extend(other.allow);
        self.ask.extend(other.ask);
        self.deny.extend(other.deny);
    }

    /// What the rules say about `access` of `tool`.
    fn verdict(&self, tool: &str, access: &Access) -> Said {
        match access {
            Access::Bash {
                command,
                folder,
                unsandboxed,
            } => {
                let said = self.command_verdict(tool, &analyze(command), folder);
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
    fn command_verdict(&self, tool: &str, analysis: &Analysis, folder: &str) -> Said {
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
            // A relative path lands where the shell is, not at the top of the working folder.
            let path = normal_path(&format!("{folder}/{target}"));
            if target.starts_with('/')
                || target.starts_with('~')
                || target.contains('$')
                || path.starts_with("..")
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
        if analysis.protected {
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
                None => return Said::Silent,
            }
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

/// A question to the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskRequest {
    pub tool: String,
    /// What the call would do, on one line: the command, the path.
    pub target: String,
}

/// The owner's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allow,
    /// Allow this and the same again for the rest of the run.
    AllowAlways,
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
}

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
/// `always`/`immer` for the rest of the run (a single letter next to `y` would be one slip away), anything else no.
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

/// Judges every tool call of a run.
pub struct Gate {
    rules: Mutex<RuleSet>,
    /// The run's mode; switched once when the owner approves a plan (`set_mode`).
    mode: Mutex<Mode>,
    asker: Arc<dyn Asker>,
    /// One question at a time: the proxy's connections can ask while a command runs, and two prompts in one terminal
    /// would mix their answers.
    asking: tokio::sync::Mutex<()>,
    log: Mutex<Vec<Judgement>>,
    /// The `allowed-tools` lists that narrow this run (HS4 C3, HS6 K5, HS6b B3) — a command's, a skill's, or both.
    /// Every one of them must name a call, so a second narrowing can only take away from the first, never give
    /// back what it left out. Added through [`Gate::narrow`].
    narrowings: Vec<Narrowing>,
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
            log: Mutex::new(Vec::new()),
            narrowings: Vec::new(),
        }
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
        !denied
            && self.narrowings.iter().all(|narrowing| {
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

    /// Whether anyone can be asked at all (a terminal, the app) — `false` for a headless run.
    pub fn can_ask(&self) -> bool {
        self.asker.present()
    }

    /// Asks the owner about something that is no single call (Glut 2: leaving Plan mode with a plan): one question at
    /// a time like every ask, refused without asking when nobody is there.
    pub async fn ask_owner(&self, tool: &str, target: &str) -> Answer {
        if !self.asker.present() {
            return Answer::Deny;
        }
        let request = AskRequest {
            tool: tool.to_owned(),
            target: target.to_owned(),
        };
        let _one_at_a_time = self.asking.lock().await;
        self.asker.ask(&request).await
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
        let target = describe(access);
        if *access == Access::Own {
            return Ok(());
        }
        // C3: an `allowed-tools` list only narrows. What one of them does not name is refused here, before any
        // rule or mode is even asked; what all of them name goes on to the normal judgement below, where such a
        // rule never allows anything by itself.
        if let Some(narrowing) = self
            .narrowings
            .iter()
            .find(|narrowing| !restrict_allows(&narrowing.rules, tool, access))
        {
            let rules = narrowing
                .rules
                .iter()
                .map(Rule::text)
                .collect::<Vec<_>>()
                .join(", ");
            let reason = format!("{} allows only: {rules}", narrowing.by);
            self.log
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(Judgement {
                    tool: tool.to_owned(),
                    target,
                    allowed: false,
                    reason: reason.clone(),
                });
            return Err(format!("not permitted: {reason}"));
        }
        let said = self
            .rules
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .verdict(tool, access);
        let mode = format!("mode {}", self.mode_name());
        let (mut verdict, mut reason) = match said {
            Said::Verdict(verdict, why) => (verdict, why),
            Said::Unsure(why) => (Verdict::Ask, why),
            Said::Silent => (self.mode().fallback(access), mode.clone()),
        };
        // Plan mode writes nothing, whatever a rule says (P3) — neither with the file tools nor by a redirection.
        let writes = match access {
            Access::Edit { .. } => true,
            Access::Bash { command, .. } => !analyze(command).writes.is_empty(),
            _ => false,
        };
        if self.mode() == Mode::Plan && writes {
            (verdict, reason) = (Verdict::Deny, mode.clone());
        }
        if verdict == Verdict::Ask && self.mode().asks_nothing() {
            verdict = Verdict::Deny;
            reason = format!("{reason}; {mode} asks nothing");
        }
        let allowed = match verdict {
            Verdict::Allow => true,
            Verdict::Deny => false,
            Verdict::Ask if !self.asker.present() => {
                reason = format!("no one to ask ({reason})");
                false
            }
            Verdict::Ask => {
                let request = AskRequest {
                    tool: tool.to_owned(),
                    target: target.clone(),
                };
                let _one_at_a_time = self.asking.lock().await;
                match self.asker.ask(&request).await {
                    Answer::Allow => {
                        reason = "allowed when asked".into();
                        true
                    }
                    Answer::AllowAlways => {
                        reason = "allowed when asked, for the rest of the run".into();
                        if let Some(rule) = always_rule(tool, access) {
                            self.rules
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .allow
                                .push(rule);
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
        self.log
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(Judgement {
                tool: tool.to_owned(),
                target,
                allowed,
                reason: reason.clone(),
            });
        if allowed {
            Ok(())
        } else {
            Err(format!("not permitted: {reason}"))
        }
    }

    /// The judgements since the last call, oldest first.
    pub fn take_log(&self) -> Vec<Judgement> {
        std::mem::take(&mut *self.log.lock().unwrap_or_else(|poison| poison.into_inner()))
    }
}

/// The rules and mode a caller adds to the settings files: flags (P7) and a role's `permissions` (P1).
#[derive(Debug, Clone, Default)]
pub struct GateOptions {
    /// The mode from a flag; before every file's.
    pub mode: Option<Mode>,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
    /// The project's `.axh/` files count (P5).
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
    /// The owner's `effort` from `~/.axh/settings.json` (Glut effort, E1): the run's unless a flag, a skill or a
    /// command names one.
    pub effort: Option<crate::config::Effort>,
    /// The owner's refusal limits from `~/.axh/settings.json` (#112, R6); the defaults without.
    pub refusals: crate::config::RefusalLimits,
    /// Whether runs compact themselves (#111, K6) and the context windows the owner named (K2).
    pub compact_auto: bool,
    pub context_windows: std::collections::HashMap<String, u32>,
    /// The owner's prices by model id (`docs/plans/glut-cards.md`, C6), for a budget in money.
    pub prices: std::collections::HashMap<String, crate::config::Prices>,
}

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
            notes.push(
                "the project's .axh/ allow rules and mode are left out: this folder is not trusted",
            );
        }
        if self.project_hooks_skipped {
            notes.push(
                "the project's own .axh/ hooks are not run: only ~/.axh/settings.json hooks run",
            );
        }
        if !self.sandbox.enabled || !crate::sandbox::available() {
            notes.push("commands run without a sandbox: only the permission rules hold them");
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
    })
}

/// The rule an "always" answer adds: exactly this command, this path, or this tool.
fn always_rule(tool: &str, access: &Access) -> Option<Rule> {
    if let Access::Bash { command, .. } = access {
        // Only one plain command is made a rule: a compound or unreadable one would be asked again anyway, and an exact
        // rule for one part of it could later allow that part inside another compound.
        let analysis = analyze(command);
        let plain = !analysis.is_opaque()
            && analysis.parts.len() == 1
            && analysis.parts[0].assignments.is_empty();
        if !plain {
            return None;
        }
    }
    let text = match access {
        // `*` in a command would widen the rule beyond what was asked; such a command is asked again next time.
        Access::Bash { command, .. } if !command.contains('*') => format!("Bash({command})"),
        Access::Bash { .. } => return None,
        Access::Edit { path } => format!("Edit({})", glob_escape(&normal_path(path))),
        Access::Read { path } => format!("Read({})", glob_escape(&normal_path(path))),
        Access::Network { host } => format!("Network({host})"),
        // "always" for an MCP call is the tool's own rule, which names it exactly (M5).
        Access::Mcp { server, tool } => format!("mcp__{server}__{tool}"),
        Access::Own | Access::Other => tool.to_owned(),
    };
    Rule::parse(&text).ok()
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
    matches!(only.verdict(tool, access), Said::Verdict(Verdict::Allow, _))
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

    #[tokio::test]
    async fn an_ask_goes_to_the_asker_and_always_holds_for_the_rest_of_the_run() {
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
            log[0].reason.contains("rest of the run")
                && log[1].reason.contains("rule Bash(make build)")
        );
        assert!(gate.take_log().is_empty());
    }

    #[tokio::test]
    async fn headless_asks_are_refused_and_own_state_is_free() {
        let gate = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        let refused = gate.judge("bash", &bash("ls")).await.unwrap_err();
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

    #[test]
    fn an_always_rule_matches_exactly_what_was_asked() {
        let rule = always_rule("bash", &bash("make build")).unwrap();
        assert!(rule.matches_words(&analyze("make build").parts[0].words));
        assert!(
            !rule.matches_words(
                &analyze("make build && rm x")
                    .parts
                    .iter()
                    .flat_map(|p| p.words.clone())
                    .collect::<Vec<_>>()
            )
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
            "axh-gate-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let (home, project) = (base.join("home"), base.join("project"));
        std::fs::create_dir_all(home.join(".axh")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            home.join(".axh/settings.json"),
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
            said(&set, "bash", bash("echo x > .axh/settings.json")),
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
            "asked, and the asker said yes"
        );
        assert_eq!(
            *asker.1.lock().unwrap(),
            1,
            "only the unreadable command in auto mode was asked"
        );
    }

    #[test]
    fn invisible_characters_never_reach_the_terminal() {
        assert_eq!(printable("rm\u{202E}fdp.exe\u{1b}[2J"), "rm fdp.exe [2J");
        assert_eq!(printable("a\u{200B}b"), "a b");
    }

    #[test]
    fn always_is_offered_only_for_one_plain_command() {
        assert!(always_rule("bash", &bash("make build")).is_some());
        assert!(always_rule("bash", &bash("a && b")).is_none());
        assert!(always_rule("bash", &bash("FOO=1 make")).is_none());
        assert!(always_rule("bash", &bash("echo $(x)")).is_none());
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
        };
        assert_eq!(
            said(&set, "bash", out("npm install")),
            Some(Verdict::Ask),
            "the allow rule does not carry over"
        );
        assert_eq!(said(&set, "bash", out("rm x")), Some(Verdict::Deny));
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
        let plan = narrowed(Mode::Plan, &["Bash(git status:*)"]);
        let refused = plan.judge("bash", &git_status).await.unwrap_err();
        assert!(refused.contains("mode plan"), "{refused}");
    }

    /// A skill run started with a slash command (`axh run --skill s "/cmd"`): both `allowed-tools` lists narrow
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
