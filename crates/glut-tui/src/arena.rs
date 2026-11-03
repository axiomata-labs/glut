//! The model arena (T61, `docs/plans/glut-tui.md`): the owner's task on two to four models at once, each in a pane
//! and a worktree of its own that starts from the owner's folder as it is, in `auto`, under limits that stop it; then
//! their numbers side by side and the winner's changes taken into the owner's folder. Cloud models run together, the
//! local ones (Ollama, LM Studio) one after another once the others are done, so none measures another's load.

use std::path::Path;
use std::time::{Duration, Instant};

use glut::providers::{Providers, models};

/// The fewest and the most models an arena runs.
pub const FEWEST: usize = 2;
pub const MOST: usize = 4;
/// How long a model may work on the task before it is stopped.
pub const TIME_LIMIT: Duration = Duration::from_secs(30 * 60);
/// What a model may spend on the task, in dollars, before it is stopped.
pub const COST_LIMIT: f64 = 2.0;

/// A model taking part, by its provider's id and its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contender {
    pub provider: String,
    pub model: String,
    /// It runs on this machine: one local model at a time.
    pub local: bool,
}

impl Contender {
    /// `provider/model`, as the input and the kept list name it.
    pub fn named(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

/// `/arena <model>, <model>: <task>`, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub contenders: Vec<Contender>,
    pub task: String,
}

/// A provider's models as the arena looks names up in them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub provider: String,
    pub local: bool,
    /// `None`: Glut keeps no list of its models yet; a name given with the provider is taken as it is.
    pub models: Option<Vec<String>>,
}

/// Every configured provider's models, from the lists Glut keeps.
pub fn listed(home: &Path) -> Vec<Listed> {
    let Ok(list) = Providers::load(home) else {
        return Vec::new();
    };
    list.providers
        .iter()
        .map(|(id, entry)| {
            let root = entry.root().trim_end_matches('/');
            Listed {
                provider: id.clone(),
                local: is_local(root),
                models: models::cached(home, id, root)
                    .map(|list| list.models.into_iter().map(|model| model.id).collect()),
            }
        })
        .collect()
}

/// Whether an API root is this machine's own.
pub fn is_local(root: &str) -> bool {
    let rest = root.split_once("://").map_or(root, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    }
    .to_lowercase();
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// The model `word` names among `lists`: `provider/model` when the first part is a provider's id, else a model's
/// whole id, else the one id that ends in `/word` or holds it.
///
/// # Errors
///
/// No model of that name, or several.
pub fn resolve(lists: &[Listed], word: &str) -> Result<Contender, String> {
    let word = word.trim();
    let contender = |listed: &Listed, model: &str| Contender {
        provider: listed.provider.clone(),
        model: model.to_owned(),
        local: listed.local,
    };
    if let Some((provider, model)) = word.split_once('/')
        && let Some(listed) = lists.iter().find(|listed| listed.provider == provider)
    {
        let known = listed
            .models
            .as_ref()
            .is_none_or(|models| models.iter().any(|id| id == model));
        if known && !model.is_empty() {
            return Ok(contender(listed, model));
        }
    }
    let lowered = word.to_lowercase();
    let tests: [&dyn Fn(&str) -> bool; 3] = [
        &|id: &str| id == word,
        &|id: &str| id.to_lowercase().ends_with(&format!("/{lowered}")),
        &|id: &str| id.to_lowercase().contains(&lowered),
    ];
    for test in tests {
        let found: Vec<Contender> = lists
            .iter()
            .flat_map(|listed| {
                listed
                    .models
                    .iter()
                    .flatten()
                    .filter(|id| test(id))
                    .map(|id| contender(listed, id))
            })
            .collect();
        match found.len() {
            0 => continue,
            1 => return Ok(found.into_iter().next().expect("one")),
            _ => {
                let names: Vec<String> = found.iter().take(4).map(Contender::named).collect();
                return Err(tr!(
                    arena_ambiguous,
                    word = crate::transcript::clean(word),
                    names = names.join(", ")
                ));
            }
        }
    }
    Err(tr!(arena_unknown, word = crate::transcript::clean(word)))
}

/// `/arena`'s words after the command: the models, a colon and a space, the task.
///
/// # Errors
///
/// No task, fewer than [`FEWEST`] or more than [`MOST`] models, a model not found.
pub fn parse(lists: &[Listed], rest: &str) -> Result<Asked, String> {
    // A colon with a space after it: a model's own (`qwen3:8b`, `…:free`) has none.
    let Some((names, task)) = rest.split_once(": ") else {
        return Err(tr!(arena_usage).into());
    };
    let task = task.trim();
    if task.is_empty() {
        return Err(tr!(arena_usage).into());
    }
    let words: Vec<&str> = names
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .collect();
    if !(FEWEST..=MOST).contains(&words.len()) {
        return Err(tr!(arena_count, fewest = FEWEST, most = MOST));
    }
    let contenders = words
        .into_iter()
        .map(|word| resolve(lists, word))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Asked {
        contenders,
        task: task.to_owned(),
    })
}

/// Where a model of the arena stands.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Its worktree and pane not made yet.
    Waiting,
    /// Its pane's session being built.
    Building,
    /// Its pane stands; a local model waits for its turn.
    Ready,
    /// It works on the task.
    Working,
    /// Its turn ended, so.
    Done(glut::FinishReason),
    /// A limit stopped it, or it never started: why.
    Aborted(String),
}

impl State {
    /// Whether it is over: done, or aborted.
    pub fn over(&self) -> bool {
        matches!(self, State::Done(_) | State::Aborted(_))
    }
}

/// A model of the arena.
#[derive(Debug, Clone)]
pub struct Member {
    pub contender: Contender,
    pub state: State,
    /// Its worktree, and the state it started from (git's tree id).
    pub tree: Option<crate::worktree::Tree>,
    pub start: Option<String>,
    /// Its pane's session id.
    pub pane: Option<u64>,
    /// When it was given the task, and how long it took.
    pub began: Option<Instant>,
    pub took: Option<Duration>,
    /// Its worktree as git's tree id once every model was over: the card's numbers and diff and a take are read
    /// from it, not from what a check wrote there since.
    pub snapshot: Option<String>,
}

/// What a member spent, read from its pane when the arena is compared.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spent {
    pub cost: Option<f64>,
    pub tokens: u64,
    pub calls: u32,
    pub tools: u32,
}

/// An arena under way, or done and waiting for the owner's pick.
#[derive(Debug, Clone)]
pub struct Arena {
    pub task: String,
    /// When it started, as its branches name it (`20261102-1830`).
    pub stamp: String,
    pub members: Vec<Member>,
    /// The pane it was started from, where its card is shown.
    pub home: u64,
    /// Its comparison was shown.
    pub compared: bool,
    /// The member whose changes went into the owner's folder.
    pub taken: Option<usize>,
    /// Each member's check (`/arena check`), in the members' order, and the stream id of the one running.
    pub checks: Vec<Check>,
    pub check_ids: Vec<Option<String>>,
    /// Why each member's last check failed, in a line: the last its output said (a bench keeps it with the result).
    pub check_said: Vec<Option<String>>,
    /// The member the owner was shown for `/arena take`, and its worktree as git's tree id then.
    pub offered: Option<(usize, String)>,
    /// The repository of the folder it started from, and its `HEAD` then: a winner is taken only while it stands.
    pub repo: Option<crate::worktree::Repo>,
    pub head: String,
    /// What stops a model: its time and its spending (a bench task's own, else [`TIME_LIMIT`] and [`COST_LIMIT`]).
    pub time_limit: Duration,
    pub cost_limit: f64,
    /// Its checks were started (a bench task's, after every model was over).
    pub checking: bool,
}

impl Arena {
    pub fn new(asked: Asked, home: u64) -> Self {
        let count = asked.contenders.len();
        Arena {
            checks: vec![Check::None; count],
            check_ids: vec![None; count],
            check_said: vec![None; count],
            task: asked.task,
            stamp: chrono::Local::now().format("%Y%m%d-%H%M%S").to_string(),
            members: asked
                .contenders
                .into_iter()
                .map(|contender| Member {
                    contender,
                    state: State::Waiting,
                    tree: None,
                    start: None,
                    pane: None,
                    began: None,
                    took: None,
                    snapshot: None,
                })
                .collect(),
            home,
            compared: false,
            taken: None,
            offered: None,
            repo: None,
            head: String::new(),
            time_limit: TIME_LIMIT,
            cost_limit: COST_LIMIT,
            checking: false,
        }
    }

    /// The branch of member `at`'s worktree.
    pub fn branch(&self, at: usize) -> String {
        format!("glut/arena-{}-{}", self.stamp, at + 1)
    }

    /// The member working in pane `id`.
    pub fn member_of(&mut self, id: u64) -> Option<&mut Member> {
        self.members
            .iter_mut()
            .find(|member| member.pane == Some(id))
    }

    /// The next member whose task may go out now, once every member's pane stands: every cloud model at once; a local
    /// one only once no other model works and every cloud model is over — a local model is measured on a machine
    /// nothing else loads.
    pub fn next_due(&self) -> Option<usize> {
        // Every worktree is made before any model works: none of them changes the repository another is made from.
        if self
            .members
            .iter()
            .any(|member| matches!(member.state, State::Waiting | State::Building))
        {
            return None;
        }
        let cloud_over = self
            .members
            .iter()
            .filter(|member| !member.contender.local)
            .all(|member| member.state.over());
        let any_working = self
            .members
            .iter()
            .any(|member| member.state == State::Working);
        self.members.iter().position(|member| {
            member.state == State::Ready
                && (!member.contender.local || (cloud_over && !any_working))
        })
    }

    /// Whether every member is over.
    pub fn over(&self) -> bool {
        self.members.iter().all(|member| member.state.over())
    }
}

/// Why member's work must stop now: past the time or the cost limit; `None` while within both.
pub fn past_limit(
    time_limit: Duration,
    cost_limit: f64,
    began: Instant,
    cost: Option<f64>,
) -> Option<String> {
    if began.elapsed() >= time_limit {
        return Some(tr!(arena_over_time, minutes = time_limit.as_secs() / 60));
    }
    match cost {
        Some(cost) if cost >= cost_limit => {
            Some(tr!(arena_over_cost, dollars = format!("{cost_limit:.2}")))
        }
        _ => None,
    }
}

/// `4:07` — minutes and seconds.
pub fn clock(took: Duration) -> String {
    let secs = took.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Where a member's check command stands (the owner's `/arena check`, run in its worktree's sandbox).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Check {
    /// Not run.
    #[default]
    None,
    /// Running now: no second one starts in that worktree.
    Running,
    /// It ended with success.
    Passed,
    /// It failed, timed out or could not run.
    Failed,
}

impl Check {
    /// Its mark in the card: nothing, `…`, `✓`, `✗`.
    pub fn mark(self) -> &'static str {
        match self {
            Check::None => "—",
            Check::Running => "…",
            Check::Passed => "✓",
            Check::Failed => "✗",
        }
    }
}

/// A member's numbers as the comparison shows them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// `provider/model`, cleaned.
    pub name: String,
    /// How it ended, in words.
    pub state: String,
    /// Whether it answered (else a limit, an error or a refusal ended it).
    pub answered: bool,
    /// The time, cost, tokens, calls and changes as shown.
    pub took: String,
    pub cost: String,
    pub tokens: String,
    pub calls: String,
    pub changed: String,
    /// The time, cost and tokens as numbers, to find the best of each.
    pub seconds: Option<u64>,
    pub dollars: Option<f64>,
    pub token_count: u64,
}

/// The best of the models that answered: the fastest, the cheapest, the one with the fewest tokens — each by its
/// place, `None` where no answered model has the number.
pub fn best(rows: &[Row]) -> [Option<usize>; 3] {
    let answered = || rows.iter().enumerate().filter(|(_, row)| row.answered);
    let fastest = answered()
        .filter_map(|(at, row)| row.seconds.map(|seconds| (at, seconds)))
        .min_by_key(|(_, seconds)| *seconds)
        .map(|(at, _)| at);
    let cheapest = answered()
        .filter_map(|(at, row)| row.dollars.map(|dollars| (at, dollars)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(at, _)| at);
    let leanest = answered()
        .filter(|(_, row)| row.token_count > 0)
        .min_by_key(|(_, row)| row.token_count)
        .map(|(at, _)| at);
    [fastest, cheapest, leanest]
}

/// Each member's row, from what it spent and changed.
pub fn rows(
    arena: &Arena,
    spent: &[Spent],
    changes: &[Result<crate::worktree::Changes, String>],
) -> Vec<Row> {
    arena
        .members
        .iter()
        .enumerate()
        .map(|(at, member)| {
            let (state, answered) = match &member.state {
                State::Done(glut::FinishReason::Answered) => (tr!(arena_done).to_owned(), true),
                State::Done(reason) => (format!("{reason:?}"), false),
                State::Aborted(why) => (crate::transcript::clean(why), false),
                _ => ("…".to_owned(), false),
            };
            let spent = spent.get(at).copied().unwrap_or_default();
            Row {
                name: crate::transcript::clean(&member.contender.named()),
                state,
                answered,
                took: member.took.map_or_else(|| "—".to_owned(), clock),
                cost: spent
                    .cost
                    .map_or_else(|| "—".to_owned(), |cost| format!("${cost:.2}")),
                tokens: tr!(
                    arena_tokens,
                    tokens = crate::looping::tokens_shown(spent.tokens)
                ),
                calls: tr!(arena_calls, calls = spent.calls, tools = spent.tools),
                changed: match changes.get(at) {
                    Some(Ok(changes)) => tr!(
                        arena_changed,
                        files = changes.files,
                        added = changes.added,
                        removed = changes.removed
                    ),
                    Some(Err(reason)) => crate::transcript::clean(reason),
                    None => String::new(),
                },
                seconds: member.took.map(|took| took.as_secs()),
                dollars: spent.cost,
                token_count: spent.tokens,
            }
        })
        .collect()
}

/// The longest reason a failed check is told by.
const SAID_CHARS: usize = 100;

/// Why a check failed, in one line: the last its output said but the shell's own `[exit code …]` note, without the
/// `error: ` the shell puts before a failing command's output; a check that said nothing is told so.
pub fn check_reason(output: &str) -> String {
    let line = output
        .lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty() && !(line.starts_with("[exit code ") && line.ends_with(']')))
        .unwrap_or_default();
    let line = line.strip_prefix("error:").unwrap_or(line).trim();
    if line.is_empty() {
        return tr!(bench_check_silent).to_owned();
    }
    crate::transcript::clean(&crate::transcript::cut(line, SAID_CHARS))
}

/// The comparison's lines for the stream: one per member, its numbers in columns, then what the owner can do. A
/// bench's task gives its checks: each line then leads with its result, and no hint follows, as the bench ends the
/// arena itself.
pub fn compared(arena: &Arena, rows: &[Row], checks: Option<&[Check]>) -> String {
    let wide = rows
        .iter()
        .map(|row| row.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines = vec![tr!(
        arena_compared,
        task = crate::transcript::clean(&crate::transcript::cut(&arena.task, 70))
    )];
    for (at, row) in rows.iter().enumerate() {
        let check = checks.map_or_else(String::new, |checks| {
            let mark = match checks.get(at) {
                Some(Check::Passed) => "✓",
                Some(Check::Failed) => "✗",
                _ => "—",
            };
            format!("{} {mark}  ", tr!(arena_check_label))
        });
        lines.push(format!(
            "  {n}  {name:<wide$}  {check}{mark} {state}  {took}  {cost}  {tokens}  {calls}  {changed}",
            n = at + 1,
            name = row.name,
            mark = if row.answered { "✓" } else { "✗" },
            state = row.state,
            took = row.took,
            cost = row.cost,
            tokens = row.tokens,
            calls = row.calls,
            changed = row.changed,
        ));
    }
    let [fastest, cheapest, leanest] = best(rows);
    if let (Some(fastest), Some(leanest)) = (fastest, leanest) {
        let cheapest = cheapest.map_or_else(|| "—".to_owned(), |at| (at + 1).to_string());
        lines.push(tr!(
            arena_best,
            fastest = fastest + 1,
            cheapest = cheapest,
            leanest = leanest + 1
        ));
    }
    if checks.is_none() {
        lines.push(tr!(arena_next).to_owned());
    }
    lines.join("\n")
}

/// What a key on the comparison card does beyond moving in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    None,
    Close,
    /// Run the check command in every worktree.
    Check,
    /// Take this member's changes: shown and asked in the pane the arena started from.
    Take(usize),
}

/// The comparison card over the screen (T61 step 1b): every model's numbers and check, below them one model's diff
/// — or two side by side.
#[derive(Debug, Clone)]
pub struct Card {
    pub rows: Vec<Row>,
    /// Each member's diff lines, cleaned for the terminal once, each with what it is — or why there is none.
    pub diffs: Vec<Vec<(Sign, String)>>,
    /// When it came up: a key before [`SETTLE`] has passed starts nothing.
    pub opened: Instant,
    /// Whether the last frame showed it: a card not drawn (too small a window, a question open) takes no key.
    pub drawn: bool,
    pub selected: usize,
    /// Two diffs side by side: the selected one and the next.
    pub side: bool,
    /// Lines the diff is scrolled down.
    pub scroll: usize,
    /// The diff's height at the last frame, for a page.
    pub page: usize,
}

/// What a line of a diff is, as the card colours it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    /// A file's header, or a note in its place.
    Head,
    /// A hunk's `@@` line.
    Hunk,
    Added,
    Removed,
    Same,
}

/// `diff`'s lines cleaned and told apart by where they stand, not by how they look: inside a hunk a removed line
/// that reads `--- …` is still a removed line.
fn signed(diff: &str) -> Vec<(Sign, String)> {
    let mut in_hunk = false;
    diff.lines()
        .map(|line| {
            let sign = if line.starts_with("diff --git ") {
                in_hunk = false;
                Sign::Head
            } else if line.starts_with("@@") {
                in_hunk = true;
                Sign::Hunk
            } else if !in_hunk {
                Sign::Head
            } else {
                match line.chars().next() {
                    Some('+') => Sign::Added,
                    Some('-') => Sign::Removed,
                    _ => Sign::Same,
                }
            };
            (sign, crate::transcript::clean(line))
        })
        .collect()
}

/// How long after the card came up a key starts nothing: one typed for what stood there before must not.
pub const SETTLE: Duration = Duration::from_millis(400);

impl Card {
    /// A card of these rows and diffs (each member's text, or why there is none), cleaned for the terminal now.
    pub fn new(rows: Vec<Row>, diffs: Vec<Result<String, String>>, selected: usize) -> Self {
        let diffs = diffs
            .into_iter()
            .map(|diff| match diff {
                Ok(diff) if diff.trim().is_empty() => {
                    vec![(Sign::Head, tr!(arena_nothing).to_owned())]
                }
                Ok(diff) => signed(&diff),
                Err(reason) => vec![(Sign::Head, crate::transcript::clean(&reason))],
            })
            .collect();
        let selected = selected.min(rows.len().saturating_sub(1));
        Card {
            rows,
            diffs,
            opened: Instant::now(),
            drawn: false,
            selected,
            side: false,
            scroll: 0,
            page: 0,
        }
    }

    /// A key on the card: ←/→ the model, ↑/↓, PgUp/PgDn, Home/End the diff, `s` side by side, `c` the check, `t`
    /// take, Esc, `q` or ⌃C close. Only a plain press counts (Shift at most); `c` and `t` not before [`SETTLE`].
    pub fn key(&mut self, key: crossterm::event::KeyEvent) -> CardAction {
        use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
        if key.kind != KeyEventKind::Press {
            return CardAction::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return if key.code == KeyCode::Char('c') {
                CardAction::Close
            } else {
                CardAction::None
            };
        }
        if !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
            return CardAction::None;
        }
        let settled = self.opened.elapsed() >= SETTLE;
        let code = key.code;
        let count = self.rows.len().max(1);
        let page = self.page.max(1);
        match code {
            KeyCode::Esc | KeyCode::Char('q') => return CardAction::Close,
            KeyCode::Right | KeyCode::Tab => {
                self.selected = (self.selected + 1) % count;
                self.scroll = 0;
            }
            KeyCode::Left | KeyCode::BackTab => {
                self.selected = (self.selected + count - 1) % count;
                self.scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll += 1,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll += page,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = usize::MAX / 2,
            KeyCode::Char('s') => self.side = !self.side && count > 1,
            KeyCode::Char('c') if settled => return CardAction::Check,
            KeyCode::Char('t') if settled => return CardAction::Take(self.selected),
            _ => {}
        }
        CardAction::None
    }

    /// The member shown beside the selected one.
    pub fn beside(&self) -> usize {
        (self.selected + 1) % self.rows.len().max(1)
    }

    /// Member `at`'s diff lines.
    pub fn diff_lines(&self, at: usize) -> &[(Sign, String)] {
        self.diffs.get(at).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Vec<Listed> {
        vec![
            Listed {
                provider: "openrouter".into(),
                local: false,
                models: Some(vec![
                    "anthropic/claude-sonnet-4.5".into(),
                    "openai/gpt-5".into(),
                    "openai/gpt-5-mini".into(),
                ]),
            },
            Listed {
                provider: "ollama".into(),
                local: true,
                models: Some(vec!["qwen3:8b".into()]),
            },
            Listed {
                provider: "zen".into(),
                local: false,
                models: None,
            },
        ]
    }

    #[test]
    fn a_name_is_found_by_provider_whole_id_end_or_part_and_two_matches_are_said() {
        let lists = lists();
        let found = resolve(&lists, "openrouter/openai/gpt-5").unwrap();
        assert_eq!(
            (found.provider.as_str(), found.model.as_str()),
            ("openrouter", "openai/gpt-5")
        );
        assert_eq!(resolve(&lists, "qwen3:8b").unwrap().provider, "ollama");
        assert!(resolve(&lists, "qwen3:8b").unwrap().local);
        assert_eq!(
            resolve(&lists, "gpt-5").unwrap().model,
            "openai/gpt-5",
            "the whole last part first"
        );
        assert_eq!(
            resolve(&lists, "sonnet").unwrap().model,
            "anthropic/claude-sonnet-4.5"
        );
        assert!(resolve(&lists, "gpt").is_err(), "two models hold it");
        assert!(resolve(&lists, "llama").is_err());
        // A provider without a kept list takes the name given with it.
        assert_eq!(
            resolve(&lists, "zen/big-pickle").unwrap().model,
            "big-pickle"
        );
    }

    #[test]
    fn the_line_needs_two_to_four_models_a_colon_and_a_task() {
        let lists = lists();
        let asked = parse(&lists, "sonnet, qwen3:8b: fix the failing test").unwrap();
        assert_eq!(asked.contenders.len(), 2);
        assert_eq!(asked.task, "fix the failing test");
        assert!(parse(&lists, "sonnet: alone").is_err());
        assert!(parse(&lists, "sonnet, gpt-5").is_err(), "no task");
        assert!(parse(&lists, "sonnet, gpt-5: ").is_err());
        assert!(parse(&lists, "sonnet, gpt-5, gpt-5-mini, qwen3:8b, zen/x: five").is_err());
        // The same model twice is allowed.
        assert!(parse(&lists, "sonnet, sonnet: twice").is_ok());
    }

    #[test]
    fn the_card_moves_between_models_scrolls_and_says_what_a_plain_settled_key_asks_for() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let mut card = Card::new(
            vec![Row::default(), Row::default(), Row::default()],
            vec![
                Ok("+a\n".into()),
                Ok(String::new()),
                Err("too\x07 big".into()),
            ],
            7,
        );
        assert_eq!(card.selected, 2, "a selection past the rows is the last");
        card.page = 10;
        assert_eq!(
            card.key(press(KeyCode::Char('t'))),
            CardAction::None,
            "too soon after it came up"
        );
        card.opened -= SETTLE;
        assert_eq!(card.key(press(KeyCode::Right)), CardAction::None);
        assert_eq!(card.selected, 0, "round the models");
        card.key(press(KeyCode::PageDown));
        assert_eq!(card.scroll, 10);
        card.key(press(KeyCode::Left));
        assert_eq!(
            (card.selected, card.scroll),
            (2, 0),
            "another model starts at its top"
        );
        card.key(press(KeyCode::Char('s')));
        assert!(card.side);
        assert_eq!(card.beside(), 0);
        assert_eq!(card.key(press(KeyCode::Char('t'))), CardAction::Take(2));
        assert_eq!(card.key(press(KeyCode::Char('c'))), CardAction::Check);
        let mut held = press(KeyCode::Char('c'));
        held.kind = KeyEventKind::Repeat;
        assert_eq!(
            card.key(held),
            CardAction::None,
            "a held key starts nothing"
        );
        let ctrl = |code| KeyEvent::new(code, KeyModifiers::CONTROL);
        assert_eq!(card.key(ctrl(KeyCode::Char('t'))), CardAction::None);
        assert_eq!(card.key(ctrl(KeyCode::Char('c'))), CardAction::Close);
        assert_eq!(card.key(press(KeyCode::Esc)), CardAction::Close);
        assert_eq!(
            card.diff_lines(0),
            [(Sign::Head, "+a".to_owned())],
            "no hunk yet"
        );
        assert_eq!(
            card.diff_lines(1),
            [(Sign::Head, tr!(arena_nothing).to_owned())]
        );
        assert_eq!(
            card.diff_lines(2),
            [(Sign::Head, "too  big".to_owned())],
            "a control character is no terminal's command"
        );
        let diff =
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n--- a comment\n+++ more\n same\n";
        let signs: Vec<Sign> = signed(diff).into_iter().map(|(sign, _)| sign).collect();
        assert_eq!(
            signs,
            [
                Sign::Head,
                Sign::Head,
                Sign::Head,
                Sign::Hunk,
                Sign::Removed,
                Sign::Added,
                Sign::Same
            ]
        );
        let alone = Card::new(vec![Row::default()], vec![Ok(String::new())], 0);
        let mut alone = alone;
        alone.key(press(KeyCode::Char('s')));
        assert!(!alone.side, "one model has nothing beside it");
    }

    #[test]
    fn the_best_are_found_among_the_models_that_answered() {
        let row = |answered, seconds, dollars, tokens| Row {
            answered,
            seconds: Some(seconds),
            dollars,
            token_count: tokens,
            ..Row::default()
        };
        let rows = [
            row(true, 26, Some(0.02), 50_000),
            row(true, 7, Some(0.03), 27_000),
            row(false, 1, Some(0.0), 10),
        ];
        assert_eq!(
            best(&rows),
            [Some(1), Some(0), Some(1)],
            "the aborted one wins nothing"
        );
        assert_eq!(best(&[row(true, 3, None, 5)]), [Some(0), None, Some(0)]);
    }

    #[test]
    fn a_bench_task_is_told_with_its_checks_and_without_the_owners_next_steps() {
        let asked = Asked {
            contenders: Vec::new(),
            task: "t".into(),
        };
        let arena = Arena::new(asked, 0);
        let rows = [
            Row {
                answered: true,
                ..Row::default()
            },
            Row::default(),
        ];
        let free = compared(&arena, &rows, None);
        assert!(free.ends_with(tr!(arena_next)));
        let bench = compared(&arena, &rows, Some(&[Check::Passed, Check::None]));
        assert!(
            !bench.contains(tr!(arena_next)),
            "a bench ends its arena itself"
        );
        let lines: Vec<&str> = bench.lines().collect();
        assert!(lines[1].contains(&format!("{} ✓", tr!(arena_check_label))));
        assert!(lines[2].contains(&format!("{} —", tr!(arena_check_label))));
    }

    #[test]
    fn a_failed_check_is_told_by_its_last_line() {
        assert_eq!(
            check_reason(
                "error: running 3 tests\ntest result: FAILED. 2 passed; 1 failed\n\n[exit code 101]"
            ),
            "test result: FAILED. 2 passed; 1 failed"
        );
        assert_eq!(check_reason("error: only\n[exit code 1]"), "only");
        assert_eq!(
            check_reason("error: \n[exit code 1]"),
            tr!(bench_check_silent)
        );
    }

    #[test]
    fn only_this_machine_counts_as_local() {
        assert!(is_local("http://localhost:11434/v1"));
        assert!(is_local("http://127.0.0.1:1234/v1"));
        assert!(is_local("http://[::1]:8080"));
        assert!(!is_local("https://openrouter.ai/api/v1"));
        assert!(!is_local("http://localhost.evil.com/v1"));
        assert!(is_local("http://LocalHost:11434"));
        assert!(is_local("http://user@127.0.0.2:8080/v1"));
        assert!(!is_local("http://localhost@evil.com/v1"));
    }

    fn member(local: bool, state: State) -> Member {
        Member {
            contender: Contender {
                provider: "p".into(),
                model: "m".into(),
                local,
            },
            state,
            tree: None,
            start: None,
            pane: None,
            began: None,
            took: None,
            snapshot: None,
        }
    }

    #[test]
    fn cloud_models_go_at_once_and_local_ones_one_by_one_after_them() {
        let asked = Asked {
            contenders: Vec::new(),
            task: "t".into(),
        };
        let mut arena = Arena::new(asked, 0);
        arena.members = vec![
            member(true, State::Ready),
            member(false, State::Ready),
            member(false, State::Working),
            member(true, State::Ready),
        ];
        assert_eq!(arena.next_due(), Some(1), "a cloud model first");
        arena.members[1].state = State::Working;
        assert_eq!(
            arena.next_due(),
            None,
            "local ones wait while a cloud model works"
        );
        arena.members[1].state = State::Done(glut::FinishReason::Answered);
        arena.members[2].state = State::Aborted("x".into());
        assert_eq!(arena.next_due(), Some(0));
        arena.members[0].state = State::Working;
        assert_eq!(arena.next_due(), None, "one local model at a time");
        arena.members[0].state = State::Done(glut::FinishReason::Answered);
        assert_eq!(arena.next_due(), Some(3));
        arena.members[3].state = State::Done(glut::FinishReason::Answered);
        assert!(arena.over());
    }
}
