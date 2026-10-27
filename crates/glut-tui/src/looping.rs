//! Loops while Glut is open (`docs/plans/glut-tui.md`, T60a): `/loop <interval> [fresh] [as <role>] [notify] <task>`
//! runs the task again and again in a background agent of its own, the interval counted from a round's end — rounds
//! never overlap. The screen keeps each loop with its pane ([`Loop`]) and starts the rounds; the agent's model may end
//! its loop with the tool `end_loop` once the goal is reached ([`EndLoop`]), which only a loop's own model is offered.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glut::ToolSpec;
use glut::permissions::Access;
use glut::tools::{CallContext, Tool, ToolFuture};
use serde_json::{Value, json};

/// The tool's name, as the rules and `glut::LOOP_TOOL` know it.
pub const NAME: &str = glut::LOOP_TOOL;
/// The shortest interval: a round is a model's turn, not a poll.
pub const SHORTEST: Duration = Duration::from_secs(10);
/// The longest interval: a day — a plan for later is T60b's.
pub const LONGEST: Duration = Duration::from_secs(24 * 3600);
/// The most rounds of one loop, unless `/loop rounds` or `loop.rounds` say otherwise.
pub const DEFAULT_ROUNDS: u32 = 50;
/// The most a loop's session may cost in USD, unless `/loop budget` or `loop.maxUsd` say otherwise.
pub const DEFAULT_MAX_USD: f64 = 5.0;
/// The most tokens a loop on a subscription may use, unless `/loop tokens` or `loop.maxTokens` say otherwise (PV4):
/// a plan's use has no price, its limits are counted in what it processes.
pub const DEFAULT_MAX_TOKENS: u64 = 5_000_000;
/// The most a loop's token budget may say.
pub const MOST_TOKENS: u64 = glut::permissions::settings::MAX_LOOP_TOKENS;
/// The commands a round may run: those that hand the model work. Any other one — `/quit`, `/loop`, `/close` — is the
/// owner's to give, never a loop's to repeat.
pub const COMMANDS: [&str; 3] = ["skill", "compact", "init"];
/// The most characters of a loop's reason for ending: a line for the owner, not a report.
const MOST_REASON: usize = 300;
/// The most a loop's budget may say, in USD.
pub const MOST_USD: f64 = 1_000.0;

/// A loop as `/loop` asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// How long after a round's end the next one starts.
    pub every: Duration,
    /// Each round starts a conversation of its own, never seeing the last.
    pub fresh: bool,
    /// The role its agent plays.
    pub role: Option<String>,
    /// A tile after every round, not only when one asks or fails or the loop ends.
    pub notify: bool,
    /// The message, or a command of [`COMMANDS`] with its words.
    pub task: String,
}

/// What `/loop` is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `/loop` alone: the loops running.
    List,
    /// A loop to start.
    Start(Asked),
    /// `/loop stop [name]`: that loop, or the only one.
    Stop(Option<String>),
    /// `/loop rounds [n]`: the most rounds kept, or said.
    Rounds(Option<u32>),
    /// `/loop budget [usd]`: the most a loop may cost kept, or said.
    Budget(Option<f64>),
    /// `/loop tokens [n]`: the most tokens a loop on a subscription may use kept, or said.
    Tokens(Option<u64>),
}

/// `/loop`'s words.
///
/// # Errors
///
/// What `/loop` takes, for words it does not.
pub fn parse(rest: &str) -> Result<Command, String> {
    let rest = rest.trim();
    let (first, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let after = after.trim();
    match first {
        "" => return Ok(Command::List),
        "stop" => {
            return Ok(Command::Stop((!after.is_empty()).then(|| after.to_owned())));
        }
        "rounds" if after.is_empty() => return Ok(Command::Rounds(None)),
        "rounds" => {
            return after
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=glut::permissions::settings::MAX_LOOP_ROUNDS).contains(n))
                .map(|n| Command::Rounds(Some(n)))
                .ok_or_else(|| {
                    tr!(
                        loop_rounds_bad,
                        most = glut::permissions::settings::MAX_LOOP_ROUNDS
                    )
                });
        }
        "budget" if after.is_empty() => return Ok(Command::Budget(None)),
        "budget" => {
            return after
                .trim_start_matches('$')
                .parse::<f64>()
                .ok()
                .filter(|usd| usable_usd(*usd))
                .map(|usd| Command::Budget(Some(usd)))
                .ok_or_else(|| tr!(loop_budget_bad).to_owned());
        }
        "tokens" if after.is_empty() => return Ok(Command::Tokens(None)),
        "tokens" => {
            return count(after)
                .filter(|n| usable_tokens(*n))
                .map(|n| Command::Tokens(Some(n)))
                .ok_or_else(|| tr!(loop_tokens_bad).to_owned());
        }
        _ => {}
    }
    let every = interval(first).ok_or_else(|| tr!(loop_interval_bad, word = first))?;
    let mut fresh = false;
    let mut notify = false;
    let mut role = None;
    let mut words = after;
    loop {
        let (word, more) = words.split_once(char::is_whitespace).unwrap_or((words, ""));
        match word {
            "fresh" => fresh = true,
            "notify" => notify = true,
            "as" => {
                let more = more.trim_start();
                let (name, then) = more.split_once(char::is_whitespace).unwrap_or((more, ""));
                if name.is_empty() {
                    return Err(tr!(loop_usage).to_owned());
                }
                role = Some(name.to_owned());
                words = then.trim_start();
                continue;
            }
            _ => break,
        }
        words = more.trim_start();
    }
    let task = words.trim();
    if task.is_empty() {
        return Err(tr!(loop_usage).to_owned());
    }
    allowed(task)?;
    Ok(Command::Start(Asked {
        every,
        fresh,
        role,
        notify,
        task: task.to_owned(),
    }))
}

/// Whether a round may run `task`: a message, or a command of [`COMMANDS`] — its name read as `App::command` reads
/// it, up to the first space.
///
/// # Errors
///
/// What a loop runs, for a command it does not.
pub fn allowed(task: &str) -> Result<(), String> {
    let Some(command) = task.strip_prefix('/') else {
        return Ok(());
    };
    let name = command.split_once(' ').map_or(command, |(name, _)| name);
    if COMMANDS.contains(&name) {
        Ok(())
    } else {
        Err(tr!(
            loop_command_refused,
            name = crate::transcript::clean(&crate::transcript::cut(name, 40))
        ))
    }
}

/// Whether `usd` can be a loop's budget.
fn usable_usd(usd: f64) -> bool {
    usd.is_finite() && usd > 0.0 && usd <= MOST_USD
}

/// Whether `tokens` can be a loop's token budget.
fn usable_tokens(tokens: u64) -> bool {
    (1..=MOST_TOKENS).contains(&tokens)
}

/// A count of tokens as written: `500000`, `500k`, `5M`, `1.5m`.
pub fn count(word: &str) -> Option<u64> {
    let word = word.trim();
    let (number, by) = match word.char_indices().last()? {
        (at, 'k' | 'K') => (&word[..at], 1_000.0),
        (at, 'm' | 'M') => (&word[..at], 1_000_000.0),
        _ => (word, 1.0),
    };
    let value = number.parse::<f64>().ok()? * by;
    (value.is_finite() && value >= 1.0 && value <= MOST_TOKENS as f64).then(|| value.round() as u64)
}

/// A count of tokens as the owner reads it: `5M`, `1.5M`, `500k`.
pub fn tokens_shown(tokens: u64) -> String {
    let trimmed = |value: f64| {
        let text = format!("{value:.1}");
        text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
    };
    if tokens >= 1_000_000 {
        format!("{}M", trimmed(tokens as f64 / 1e6))
    } else if tokens >= 1_000 {
        format!("{}k", trimmed(tokens as f64 / 1e3))
    } else {
        tokens.to_string()
    }
}

/// What `usage` counts against a token budget, as `glut::Budget` counts it.
pub fn tokens_of(usage: &glut::Usage) -> u64 {
    usage.budgeted_tokens()
}

/// An interval as written: `30s`, `5m`, `2h`, `1h30m` — from [`SHORTEST`] to [`LONGEST`].
pub fn interval(word: &str) -> Option<Duration> {
    let mut secs: u64 = 0;
    let mut digits = String::new();
    let mut units = 0;
    for c in word.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            _ => return None,
        };
        let n: u64 = digits.parse().ok()?;
        secs = secs.checked_add(n.checked_mul(unit)?)?;
        digits.clear();
        units += 1;
    }
    // A number without its unit says nothing sure.
    if !digits.is_empty() || units == 0 {
        return None;
    }
    let every = Duration::from_secs(secs);
    (SHORTEST..=LONGEST).contains(&every).then_some(every)
}

/// An interval as the owner reads it: `30s`, `5m`, `1h30m`.
pub fn shown(every: Duration) -> String {
    let secs = every.as_secs();
    let (hours, minutes, seconds) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let mut text = String::new();
    if hours > 0 {
        text.push_str(&format!("{hours}h"));
    }
    if minutes > 0 {
        text.push_str(&format!("{minutes}m"));
    }
    if seconds > 0 || text.is_empty() {
        text.push_str(&format!("{seconds}s"));
    }
    text
}

/// Where a loop ends by itself: what `/loop rounds`, `/loop budget` and `/loop tokens` kept, else the owner's `loop`
/// in `~/.glut/settings.json`, else the defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub rounds: u32,
    pub max_usd: f64,
    /// The most tokens it may use: only on a subscription (PV4), whose use has no price.
    pub max_tokens: Option<u64>,
}

impl Limits {
    /// The limits of a loop; `plan`: it runs on a subscription, and its tokens count too.
    pub fn of(
        prefs: &crate::prefs::Prefs,
        settings: glut::permissions::settings::LoopSection,
        plan: bool,
    ) -> Self {
        // The prefs file may be edited by hand: a value `/loop` would refuse counts as none.
        let rounds = |n: &u32| (1..=glut::permissions::settings::MAX_LOOP_ROUNDS).contains(n);
        Limits {
            rounds: prefs
                .loop_rounds
                .filter(rounds)
                .or(settings.rounds.filter(rounds))
                .unwrap_or(DEFAULT_ROUNDS),
            max_usd: prefs
                .loop_max_usd
                .filter(|usd| usable_usd(*usd))
                .or(settings.max_usd.filter(|usd| usable_usd(*usd)))
                .unwrap_or(DEFAULT_MAX_USD),
            max_tokens: plan.then(|| {
                prefs
                    .loop_max_tokens
                    .filter(|n| usable_tokens(*n))
                    .or(settings.max_tokens.filter(|n| usable_tokens(*n)))
                    .unwrap_or(DEFAULT_MAX_TOKENS)
            }),
        }
    }
}

/// Why a loop ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// Its model said the goal is reached, with why.
    Goal(String),
    /// The owner stopped it (`/loop stop`, Esc in its round).
    Stopped,
    /// A round did not end answered: how it ended.
    Failed(String),
    /// It ran its most rounds.
    Rounds(u32),
    /// Its session spent its budget.
    Budget(f64),
    /// Its rounds used the tokens they may, on a subscription.
    Tokens(u64),
}

impl Ended {
    /// Why, as the owner reads it.
    pub fn said(&self) -> String {
        match self {
            Ended::Goal(reason) => tr!(
                loop_why_goal,
                // The model's words: one line, however it wrote them.
                reason = crate::transcript::clean(&crate::transcript::cut(
                    &reason.replace(['\n', '\r'], " "),
                    200
                ))
            ),
            Ended::Stopped => tr!(loop_why_stopped).to_owned(),
            Ended::Failed(how) => tr!(loop_why_failed, reason = how),
            Ended::Rounds(n) => tr!(loop_why_rounds, n = n),
            Ended::Budget(max) => tr!(loop_why_budget, max = glut::lang::usd(*max, 2)),
            Ended::Tokens(max) => tr!(loop_why_tokens, max = tokens_shown(*max)),
        }
    }

    /// Whether the loop ended as it should: its goal reached, or its limits run out.
    pub fn well(&self) -> bool {
        !matches!(self, Ended::Failed(_))
    }
}

/// One loop as the screen keeps it, with the agent's pane.
#[derive(Debug)]
pub struct Loop {
    pub asked: Asked,
    pub limits: Limits,
    /// Rounds ended so far.
    pub rounds: u32,
    /// What its rounds cost so far, where the prices are known; `None`: nothing counted yet.
    pub spent: Option<f64>,
    /// The tokens its rounds used so far.
    pub used: u64,
    /// When the next round starts; `None` while one runs.
    pub next: Option<Instant>,
    /// What the session's cost was as the round under way started.
    pub cost_from: Option<f64>,
    /// What the session's tokens were as the round under way started.
    pub used_from: u64,
    /// How the round's last turn ended, with its answer; `None` while it runs or when it started none.
    pub last: Option<(glut::FinishReason, String)>,
    /// What the agent's `end_loop` says.
    pub link: Arc<Link>,
    /// When it started.
    pub began: Instant,
}

impl Loop {
    /// A loop whose first round starts at once.
    pub fn new(asked: Asked, limits: Limits, link: Arc<Link>) -> Self {
        link.start(&asked, limits);
        Loop {
            asked,
            limits,
            rounds: 0,
            spent: None,
            used: 0,
            next: Some(Instant::now()),
            cost_from: None,
            used_from: 0,
            last: None,
            link,
            began: Instant::now(),
        }
    }

    /// Whether a round runs.
    pub fn running(&self) -> bool {
        self.next.is_none()
    }

    /// A round is about to start: what it may still spend is set before its turn reads it.
    pub fn round_starts(&self) {
        self.link.set_left(Some(Left {
            tokens: self
                .limits
                .max_tokens
                .map(|max| max.saturating_sub(self.used)),
            usd: (self.limits.max_usd - self.spent.unwrap_or(0.0)).max(0.0),
        }));
    }

    /// The round under way began, with the session's cost and tokens then — after a fresh round's `/clear`.
    pub fn round_began(&mut self, cost: Option<f64>, used: u64) {
        self.next = None;
        self.cost_from = cost;
        self.used_from = used;
        self.last = None;
        // An `end_loop` of a turn between rounds (the owner's, in the shown pane) ends no round.
        self.link.take_ended();
    }

    /// The round under way ended, with the session's cost and tokens now: whether and why the loop ends, else when
    /// the next round starts.
    pub fn round_ended(&mut self, cost: Option<f64>, used: u64, now: Instant) -> Option<Ended> {
        self.rounds += 1;
        self.used = self
            .used
            .saturating_add(used.saturating_sub(self.used_from));
        // A turn the owner sends between rounds is no round's: it spends without the loop's limits.
        self.link.set_left(None);
        if let Some(cost) = cost {
            let spent = (cost - self.cost_from.unwrap_or(0.0)).max(0.0);
            self.spent = Some(self.spent.unwrap_or(0.0) + spent);
        }
        let ended = match self.last.take() {
            Some((glut::FinishReason::Cancelled, _)) => Some(Ended::Stopped),
            // The round's run stopped at what was left: the loop's limit, said as such.
            Some((glut::FinishReason::Budget, _)) => Some(match self.limits.max_tokens {
                Some(max) if self.used >= max => Ended::Tokens(max),
                _ => Ended::Budget(self.limits.max_usd),
            }),
            Some((reason, _)) if reason != glut::FinishReason::Answered => {
                Some(Ended::Failed(crate::app::reason(reason).to_owned()))
            }
            _ => None,
        }
        .or_else(|| self.link.take_ended().map(Ended::Goal))
        .or_else(|| (self.rounds >= self.limits.rounds).then_some(Ended::Rounds(self.rounds)))
        .or_else(|| {
            self.spent
                .filter(|spent| *spent >= self.limits.max_usd)
                .map(|_| Ended::Budget(self.limits.max_usd))
        })
        .or_else(|| {
            self.limits
                .max_tokens
                .filter(|max| self.used >= *max)
                .map(Ended::Tokens)
        });
        if ended.is_some() {
            self.link.stop();
        } else {
            self.next = Some(now + self.asked.every);
        }
        ended
    }

    /// What its rounds used, as the owner reads it: the tokens on a subscription, else the money where it counts.
    pub fn spent_shown(&self) -> String {
        match (self.limits.max_tokens, self.spent) {
            (Some(_), _) => tr!(loop_tokens_used, n = tokens_shown(self.used)),
            (None, Some(spent)) => glut::lang::usd(spent, 2),
            (None, None) => "—".to_owned(),
        }
    }

    /// The loop as `/loop` lists it.
    pub fn line(&self, name: &str, now: Instant) -> String {
        let when = match self.next {
            None => tr!(loop_round_runs).to_owned(),
            Some(at) => tr!(
                loop_next_in,
                time = shown(Duration::from_secs(
                    at.saturating_duration_since(now).as_secs()
                ))
            ),
        };
        let cost = self.spent_shown();
        tr!(
            loop_line,
            name = name,
            every = shown(self.asked.every),
            round = self.rounds,
            rounds = self.limits.rounds,
            when = when,
            cost = cost,
            task = crate::transcript::clean(&crate::transcript::cut(&self.asked.task, 60))
        )
    }
}

/// What the screen tells a session's `end_loop` about its loop, and what the model said back.
#[derive(Debug, Default)]
pub struct Link {
    state: Mutex<LinkState>,
}

#[derive(Debug, Default, Clone)]
struct LinkState {
    /// The session runs a loop: the tool is offered.
    on: bool,
    every: String,
    task: String,
    rounds: u32,
    /// The model ended the loop, with why.
    ended: Option<String>,
    /// What the round under way may still spend: its turn's run stops there, not only the next round.
    left: Option<Left>,
}

/// What a round may still spend of its loop's limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Left {
    /// Tokens, on a subscription.
    pub tokens: Option<u64>,
    /// Money, counted where the prices are known.
    pub usd: f64,
}

impl Link {
    fn lock(&self) -> std::sync::MutexGuard<'_, LinkState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn start(&self, asked: &Asked, limits: Limits) {
        *self.lock() = LinkState {
            on: true,
            every: shown(asked.every),
            task: asked.task.clone(),
            rounds: limits.rounds,
            ended: None,
            left: None,
        };
    }

    /// The loop is over: the tool is no longer offered.
    pub fn stop(&self) {
        let mut state = self.lock();
        state.on = false;
        state.ended = None;
        state.left = None;
    }

    /// What the round under way may still spend; `None` between rounds and without a loop.
    pub fn left(&self) -> Option<Left> {
        self.lock().left
    }

    fn set_left(&self, left: Option<Left>) {
        self.lock().left = left;
    }

    /// Whether `end_loop` is offered: the session runs a loop.
    pub fn on(&self) -> bool {
        self.lock().on
    }

    /// The loop ended by its model, as `end_loop` does it.
    #[cfg(test)]
    pub fn end_for_tests(&self, reason: &str) {
        self.lock().ended = Some(reason.to_owned());
    }

    fn take_ended(&self) -> Option<String> {
        self.lock().ended.take()
    }
}

/// The `end_loop` tool of one session.
pub struct EndLoop {
    link: Arc<Link>,
}

impl EndLoop {
    pub fn new(link: Arc<Link>) -> Self {
        EndLoop { link }
    }
}

impl Tool for EndLoop {
    fn spec(&self) -> ToolSpec {
        let state = self.link.lock().clone();
        // The same for every round: a description that changed would cost the prompt's cached head.
        let description = format!(
            "End the loop you run in. The owner started a loop: the same task comes to you again {} after each round \
             ends, for at most {} rounds — the task: {}. Call this only when the task itself names an end (\"until \
             the tests pass\", \"stop once the file is there\") and that end is reached, or can never be reached any \
             more; give a short reason for the owner, and the loop stops after this round. A task that is simply \
             repeated — say, count, check, report, fetch — has no end of its own: never call this for it, even though \
             one round did what it asks; the owner chose to have it again, and the rounds limit, the budget or the \
             owner end it. Never call it because a round found nothing new either.",
            state.every,
            state.rounds,
            glut::permissions::printable(&crate::transcript::cut(&state.task, 300))
        );
        ToolSpec {
            name: NAME.to_owned(),
            description,
            parameters: json!({
                "type": "object",
                "properties": {
                    "reason": { "type": "string", "description": "Why the loop ends, in a sentence." }
                },
                "required": ["reason"]
            }),
        }
    }

    fn access(&self, _arguments: &Value) -> Access {
        // It only ends what the owner started: nothing to judge.
        Access::Own
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        Box::pin(async move {
            let reason = arguments
                .get("reason")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or("end_loop needs a reason")?;
            let mut state = self.link.lock();
            if !state.on {
                return Err("this session runs no loop".to_owned());
            }
            let reason: String = reason.chars().take(MOST_REASON).collect();
            state.ended = Some(reason);
            Ok("the loop ends once this round is over — finish the round's answer".to_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interval_is_seconds_minutes_hours_or_both_within_its_bounds() {
        assert_eq!(interval("30s"), Some(Duration::from_secs(30)));
        assert_eq!(interval("5m"), Some(Duration::from_secs(300)));
        assert_eq!(interval("1h30m"), Some(Duration::from_secs(5400)));
        assert_eq!(interval("24h"), Some(LONGEST));
        for odd in [
            "",
            "5",
            "5x",
            "m",
            "9s",
            "25h",
            "1h5",
            "-5m",
            "99999999999999999999h",
        ] {
            assert_eq!(interval(odd), None, "{odd}");
        }
        assert_eq!(shown(Duration::from_secs(5400)), "1h30m");
        assert_eq!(shown(Duration::from_secs(45)), "45s");
        assert_eq!(shown(Duration::from_secs(0)), "0s");
    }

    #[test]
    fn loop_reads_its_interval_its_words_and_the_task_after_them() {
        assert_eq!(parse("").unwrap(), Command::List);
        assert_eq!(parse("stop").unwrap(), Command::Stop(None));
        assert_eq!(
            parse("stop tests").unwrap(),
            Command::Stop(Some("tests".into()))
        );
        assert_eq!(parse("rounds 20").unwrap(), Command::Rounds(Some(20)));
        assert_eq!(parse("rounds").unwrap(), Command::Rounds(None));
        assert!(parse("rounds 0").is_err() && parse("rounds many").is_err());
        assert_eq!(parse("budget $2.5").unwrap(), Command::Budget(Some(2.5)));
        assert!(parse("budget -1").is_err() && parse("budget NaN").is_err());
        assert_eq!(
            parse("5m fresh as reviewer notify look at the open PRs, as before").unwrap(),
            Command::Start(Asked {
                every: Duration::from_secs(300),
                fresh: true,
                role: Some("reviewer".into()),
                notify: true,
                task: "look at the open PRs, as before".into(),
            })
        );
        // The words stop where the task begins: a later "fresh" is the task's.
        let Command::Start(asked) = parse("1m run the tests fresh").unwrap() else {
            panic!()
        };
        assert!(!asked.fresh && asked.task == "run the tests fresh");
        assert!(parse("5m").is_err() && parse("5m fresh").is_err() && parse("5m as").is_err());
        assert!(parse("often run the tests").is_err());
    }

    #[test]
    fn a_rounds_command_is_one_that_hands_the_model_work_never_one_of_the_owners() {
        let Command::Start(asked) = parse("10m /skill review the diff").unwrap() else {
            panic!()
        };
        assert_eq!(asked.task, "/skill review the diff");
        assert!(parse("10m /compact").is_ok());
        for refused in [
            "10m /quit",
            "10m /loop 5m hi",
            "10m /close tests",
            "10m /clear",
            "10m /",
        ] {
            assert!(parse(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn limits_the_prefs_file_could_hold_but_loop_would_refuse_count_as_none() {
        let mut prefs = crate::prefs::Prefs::default();
        prefs.loop_rounds = Some(0);
        prefs.loop_max_usd = Some(f64::NAN);
        let settings = glut::permissions::settings::LoopSection {
            rounds: Some(9),
            max_usd: None,
            max_tokens: Some(0),
        };
        assert_eq!(
            Limits::of(&prefs, settings, false),
            Limits {
                rounds: 9,
                max_usd: DEFAULT_MAX_USD,
                max_tokens: None,
            }
        );
        assert_eq!(
            Limits::of(&prefs, settings, true).max_tokens,
            Some(DEFAULT_MAX_TOKENS),
            "only on a subscription, and a count loop would refuse is none"
        );
        assert!(parse("budget 5000").is_err());
        assert!(
            allowed("/skill\treview").is_err(),
            "named as App::command reads it"
        );
        assert!(allowed("/skill review").is_ok() && allowed("look at it").is_ok());
    }

    fn asked(every: u64) -> Asked {
        Asked {
            every: Duration::from_secs(every),
            fresh: false,
            role: None,
            notify: false,
            task: "run the tests".into(),
        }
    }

    #[test]
    fn a_round_that_ends_answered_sets_the_next_from_its_end_and_the_limits_end_the_loop() {
        let link = Arc::new(Link::default());
        let limits = Limits {
            rounds: 2,
            max_usd: 1.0,
            max_tokens: None,
        };
        let mut looped = Loop::new(asked(60), limits, Arc::clone(&link));
        assert!(link.on() && !looped.running());
        looped.round_began(None, 0);
        assert!(looped.running());
        looped.last = Some((glut::FinishReason::Answered, "green".into()));
        let now = Instant::now();
        assert_eq!(looped.round_ended(None, 0, now), None);
        assert_eq!(looped.next, Some(now + Duration::from_secs(60)));
        assert_eq!(looped.spent, None, "nothing counted without prices");
        looped.round_began(None, 0);
        assert_eq!(looped.round_ended(None, 0, now), Some(Ended::Rounds(2)));
        assert!(!link.on(), "the tool goes with the loop");

        // The budget counts what each round cost, from its start.
        let link = Arc::new(Link::default());
        let mut looped = Loop::new(
            asked(60),
            Limits {
                rounds: 50,
                max_usd: 1.0,
                max_tokens: None,
            },
            link,
        );
        looped.round_began(Some(0.2), 0);
        assert_eq!(looped.round_ended(Some(0.8), 0, now), None);
        looped.round_began(Some(0.8), 0);
        assert_eq!(
            looped.round_ended(Some(1.3), 0, now),
            Some(Ended::Budget(1.0))
        );
        assert!((looped.spent.unwrap() - 1.1).abs() < 1e-9);

        // On a subscription its tokens count, each round's from its start.
        let mut looped = Loop::new(
            asked(60),
            Limits {
                rounds: 50,
                max_usd: 1.0,
                max_tokens: Some(1_000),
            },
            Arc::new(Link::default()),
        );
        looped.round_began(None, 100);
        assert_eq!(looped.round_ended(None, 700, now), None);
        assert_eq!(looped.spent_shown(), tr!(loop_tokens_used, n = "600"));
        looped.round_starts();
        looped.round_began(None, 0);
        assert_eq!(
            looped.link.left(),
            Some(Left {
                tokens: Some(400),
                usd: 1.0
            }),
            "a round's run stops at what is left"
        );
        // Its run stopped there: the loop ends on its tokens, not as failed.
        looped.last = Some((glut::FinishReason::Budget, String::new()));
        assert_eq!(
            looped.round_ended(None, 400, now),
            Some(Ended::Tokens(1_000))
        );
        assert_eq!(looped.link.left(), None, "nothing left between rounds");
    }

    #[test]
    fn a_count_of_tokens_reads_as_written_and_shows_short() {
        assert_eq!(count("500000"), Some(500_000));
        assert_eq!(count("500k"), Some(500_000));
        assert_eq!(count("1.5M"), Some(1_500_000));
        assert_eq!(count("5m"), Some(5_000_000));
        for odd in ["", "0", "-5k", "k", "2000M", "nan", "5 M"] {
            assert_eq!(count(odd), None, "{odd}");
        }
        assert_eq!(
            parse("tokens 2M").unwrap(),
            Command::Tokens(Some(2_000_000))
        );
        assert_eq!(parse("tokens").unwrap(), Command::Tokens(None));
        assert!(parse("tokens lots").is_err());
        assert_eq!(tokens_shown(5_000_000), "5M");
        assert_eq!(tokens_shown(1_500_000), "1.5M");
        assert_eq!(tokens_shown(600), "600");
        assert_eq!(tokens_shown(42_000), "42k");
        let usage = glut::Usage {
            input_tokens: 1_000,
            cached_input_tokens: 800,
            output_tokens: 50,
        };
        assert_eq!(tokens_of(&usage), 250, "cache reads do not count");
    }

    #[test]
    fn a_failed_or_stopped_round_ends_the_loop_and_so_does_its_model() {
        let now = Instant::now();
        let limits = Limits {
            rounds: 50,
            max_usd: 5.0,
            max_tokens: None,
        };
        let mut looped = Loop::new(asked(60), limits, Arc::new(Link::default()));
        looped.round_began(None, 0);
        looped.last = Some((glut::FinishReason::Failed, String::new()));
        assert!(matches!(
            looped.round_ended(None, 0, now),
            Some(Ended::Failed(_))
        ));

        let mut looped = Loop::new(asked(60), limits, Arc::new(Link::default()));
        looped.round_began(None, 0);
        looped.last = Some((glut::FinishReason::Cancelled, String::new()));
        assert_eq!(looped.round_ended(None, 0, now), Some(Ended::Stopped));

        let link = Arc::new(Link::default());
        let tool = EndLoop::new(Arc::clone(&link));
        let mut looped = Loop::new(asked(60), limits, Arc::clone(&link));
        looped.round_began(None, 0);
        assert!(tool.spec().description.contains("at most 50 rounds"));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let context = CallContext::default();
        assert!(
            runtime
                .block_on(tool.call(json!({}), context.clone()))
                .is_err()
        );
        runtime
            .block_on(tool.call(json!({"reason": "all green"}), context.clone()))
            .unwrap();
        looped.last = Some((glut::FinishReason::Answered, "done".into()));
        assert_eq!(
            looped.round_ended(None, 0, now),
            Some(Ended::Goal("all green".into()))
        );
        assert!(
            runtime
                .block_on(tool.call(json!({"reason": "again"}), context))
                .is_err(),
            "no loop, no end"
        );
    }
}
