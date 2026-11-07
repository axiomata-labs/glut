//! Telling the owner a turn ended or asks while they look elsewhere (T53): through the
//! terminal's own notifications (OSC 9, kitty's OSC 99) where it is known to have them, the bell beside it always —
//! macOS's notifications only on the owner's `/notify system` and never over SSH, for a click on one opens Script
//! Editor, not the terminal. Only while the window is not in front (the terminal's focus reports); a turn's end only
//! after it ran a while, an ask at once. At most one message per [`FOLD_FOR`], what comes in between folded into it;
//! an ask never waits for that. A message names Glut, the project, the pane and the kind — never the model's or a
//! command's words, which a lock screen would show.

use std::io::{self, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use glut::permissions::settings::{NotifySection, NotifyWhen};

/// How long after one message the next waits, gathering what comes in between.
pub const FOLD_FOR: Duration = Duration::from_secs(5);
/// How many seconds a turn runs before its end is told, unless the owner says otherwise.
pub const DEFAULT_AFTER: u32 = 10;
/// The most seconds `/notify after` takes: an hour, as `notify.after` in the settings.
pub const MAX_AFTER: u32 = glut::permissions::settings::MAX_NOTIFY_AFTER;
/// The most occasions one message names; the rest are counted.
const MOST_NAMED: usize = 3;
/// The longest tool name a message names.
const MOST_TOOL_CHARS: usize = 40;
/// How long after one message an ask's waits at least: a burst of asks is one message, not one each.
const ASK_GAP: Duration = Duration::from_secs(1);
/// The longest a message's text gets, in characters: a notification shows a line or two.
const MOST_CHARS: usize = 200;

/// What the owner chose: `/notify` kept, else the settings' `notify`, else on after [`DEFAULT_AFTER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub when: NotifyWhen,
    pub after: u32,
}

static CHOICE: Mutex<Choice> = Mutex::new(Choice {
    when: NotifyWhen::On,
    after: DEFAULT_AFTER,
});

/// Sets what holds from now on: the kept choice over the settings' over the default.
pub fn set(kept_when: Option<NotifyWhen>, kept_after: Option<u32>, settings: NotifySection) {
    let choice = Choice {
        when: kept_when.or(settings.when).unwrap_or(NotifyWhen::On),
        after: kept_after
            .or(settings.after)
            .unwrap_or(DEFAULT_AFTER)
            .min(MAX_AFTER),
    };
    *CHOICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = choice;
}

/// What holds now.
pub fn choice() -> Choice {
    *CHOICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `/notify`'s word.
pub fn parse(word: &str) -> Option<NotifyWhen> {
    match word {
        "on" => Some(NotifyWhen::On),
        "asks" => Some(NotifyWhen::Asks),
        "off" => Some(NotifyWhen::Off),
        "system" => Some(NotifyWhen::System),
        _ => None,
    }
}

/// `when` as `/notify` writes it.
pub fn word(when: NotifyWhen) -> &'static str {
    match when {
        NotifyWhen::On => "on",
        NotifyWhen::Asks => "asks",
        NotifyWhen::Off => "off",
        NotifyWhen::System => "system",
    }
}

/// How a message reaches the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// OSC 9: iTerm2, Ghostty, WezTerm.
    Osc9,
    /// OSC 99, kitty's own.
    Osc99,
    /// The bell alone: the Dock or the tab marks it.
    Bell,
    /// macOS's notifications through `osascript`.
    System,
}

impl Way {
    /// The way as `/notify` names it.
    pub fn label(self) -> &'static str {
        match self {
            Way::Osc9 => "OSC 9",
            Way::Osc99 => "OSC 99",
            Way::Bell => "BEL",
            Way::System => "osascript",
        }
    }
}

/// What the environment says about the terminal, read once.
#[derive(Debug, Default, Clone)]
pub struct Env {
    pub term_program: Option<String>,
    pub term: Option<String>,
    /// Inside tmux: what is sent goes through its passthrough.
    pub tmux: bool,
    /// Over SSH: macOS's notifications would show on the other machine.
    pub ssh: bool,
    /// The marks terminals leave in the environment, which survive a tmux in between: kitty, Ghostty, iTerm2,
    /// WezTerm.
    pub kitty: bool,
    pub ghostty: bool,
    pub iterm: bool,
    pub wezterm: bool,
}

impl Env {
    pub fn read() -> Env {
        let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        let var = |name: &str| std::env::var(name).ok();
        Env {
            term_program: var("TERM_PROGRAM"),
            term: var("TERM"),
            tmux: set("TMUX"),
            ssh: set("SSH_TTY") || set("SSH_CONNECTION"),
            kitty: set("KITTY_WINDOW_ID"),
            ghostty: set("GHOSTTY_RESOURCES_DIR"),
            iterm: set("ITERM_SESSION_ID") || var("LC_TERMINAL").as_deref() == Some("iTerm2"),
            wezterm: set("WEZTERM_PANE"),
        }
    }

    /// The terminal's own way, if it is known to have one — and so to report its focus.
    fn own_way(&self) -> Option<Way> {
        let program = self
            .term_program
            .as_deref()
            .unwrap_or_default()
            .to_lowercase();
        let term = self.term.as_deref().unwrap_or_default().to_lowercase();
        if self.kitty || term.contains("kitty") {
            return Some(Way::Osc99);
        }
        let osc9 = matches!(program.as_str(), "ghostty" | "iterm.app" | "wezterm")
            || term.contains("ghostty")
            || self.ghostty
            || self.iterm
            || self.wezterm;
        osc9.then_some(Way::Osc9)
    }

    /// How a message goes out under `when`.
    pub fn way(&self, when: NotifyWhen) -> Way {
        if when == NotifyWhen::System && !self.ssh && cfg!(target_os = "macos") {
            return Way::System;
        }
        self.own_way().unwrap_or(Way::Bell)
    }

    /// Whether the terminal is known to report its focus: then the window is taken to be in front at the start,
    /// where the owner just typed `glut`.
    pub fn reports_focus(&self) -> bool {
        self.own_way().is_some()
    }
}

/// What happened, for a message or a toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Occasion {
    /// A turn ended, after `ran`; `answered`: with an answer, else it broke off (an error, refusals, the budget).
    Ended { ran: Duration, answered: bool },
    /// The gate asks the owner about a call of `tool`.
    Asks { tool: String },
}

impl Occasion {
    pub fn is_ask(&self) -> bool {
        matches!(self, Occasion::Asks { .. })
    }

    /// What it is, in words: "done after 2 min", "asks: run a command".
    pub fn said(&self) -> String {
        match self {
            Occasion::Ended { ran, answered } => {
                let time = duration(*ran);
                if *answered {
                    tr!(notify_done, time = time)
                } else {
                    tr!(notify_ended, time = time)
                }
            }
            Occasion::Asks { tool } => tr!(notify_asks, what = ask_kind(tool)),
        }
    }
}

/// The kind of call an ask is about, by its tool — never its target, which may be a secret.
fn ask_kind(tool: &str) -> String {
    match tool {
        "bash" => tr!(notify_ask_command).to_owned(),
        "write_file" | "edit_file" => tr!(notify_ask_file).to_owned(),
        glut::sandbox::proxy::NETWORK_TOOL | "web_fetch" | "web_search" => {
            tr!(notify_ask_network).to_owned()
        }
        "task" => tr!(notify_ask_agent).to_owned(),
        "memory" | "glut_file" => tr!(notify_ask_glut).to_owned(),
        tool if tool.starts_with("mcp__") => tr!(notify_ask_mcp).to_owned(),
        // Another tool's name, as far as it is a name: what else a call names stays off the lock screen.
        tool => {
            let name: String = tool
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .take(MOST_TOOL_CHARS)
                .collect();
            tr!(notify_ask_other, tool = name)
        }
    }
}

/// `ran` as a message says it: seconds under a minute, else minutes.
fn duration(ran: Duration) -> String {
    let secs = ran.as_secs();
    if secs < 60 {
        tr!(notify_secs, n = secs)
    } else {
        tr!(notify_mins, n = (secs + 30) / 60)
    }
}

/// Whether `occasion` is told by a message: by the choice, the focus and how long its turn ran. A turn's end waits for
/// [`Choice::after`] seconds of turn; an ask goes at once while the window is known to be elsewhere, and — with no
/// focus known — only once its turn ran as long as an end would need.
pub fn worth_a_message(
    choice: Choice,
    focused: Option<bool>,
    occasion: &Occasion,
    ran: Duration,
) -> bool {
    let long = ran >= Duration::from_secs(u64::from(choice.after));
    match (choice.when, occasion.is_ask()) {
        (NotifyWhen::Off, _) | (NotifyWhen::Asks, false) => false,
        _ if focused == Some(true) => false,
        (_, true) => focused == Some(false) || long,
        (_, false) => long,
    }
}

/// The screen's notifications: the window's focus, and what waits to go out.
#[derive(Debug, Default)]
pub struct Notifier {
    /// Whether the window is in front; `None` while the terminal never said.
    pub focused: Option<bool>,
    /// The occasions not yet told, each with who it is about (`None`: the only pane).
    waiting: Vec<String>,
    /// One of them is an ask: it goes at once.
    urgent: bool,
    /// When the last message went.
    last: Option<Instant>,
}

impl Notifier {
    pub fn new(env: &Env) -> Notifier {
        Notifier {
            // tmux forwards no focus reports unless its `focus-events` is on: there the window's place is unknown
            // until a first report comes — never taken to be in front, which would tell nothing at all.
            focused: (env.reports_focus() && !env.tmux).then_some(true),
            ..Notifier::default()
        }
    }

    /// The window came to the front or went: what waits to be told is old news once the owner looks.
    pub fn focus(&mut self, front: bool) {
        self.focused = Some(front);
        if front {
            self.waiting.clear();
            self.urgent = false;
        }
    }

    /// `occasion` of `who` (a pane, an agent; `None` alone on the screen) waits to be told.
    pub fn push(&mut self, who: Option<String>, occasion: &Occasion) {
        let said = occasion.said();
        self.waiting.push(match who {
            Some(who) => format!("{who}: {said}"),
            None => said,
        });
        self.urgent |= occasion.is_ask();
    }

    /// When a waiting message may go, if one waits.
    pub fn due(&self) -> Option<Instant> {
        if self.waiting.is_empty() {
            return None;
        }
        match self.last {
            Some(last) if self.urgent => Some(last + ASK_GAP),
            Some(last) => Some(last + FOLD_FOR),
            None => Some(Instant::now()),
        }
    }

    /// The message's text for `project`, if one may go `now`: everything waiting, folded into one.
    pub fn take(&mut self, project: &str, now: Instant) -> Option<String> {
        let since = |gap: Duration| self.last.is_none_or(|last| now.duration_since(last) >= gap);
        let free = since(FOLD_FOR) || (self.urgent && since(ASK_GAP));
        if self.waiting.is_empty() || !free {
            return None;
        }
        let waiting = std::mem::take(&mut self.waiting);
        self.urgent = false;
        self.last = Some(now);
        let mut parts: Vec<String> = waiting.iter().take(MOST_NAMED).cloned().collect();
        if waiting.len() > MOST_NAMED {
            parts.push(tr!(notify_more, n = waiting.len() - MOST_NAMED));
        }
        let text = format!("{project} · {}", parts.join(" · "));
        Some(plain(&text))
    }
}

/// `text` with nothing that could end the escape around it or move the terminal: control characters as spaces,
/// bidi and zero-width marks out, cut at [`MOST_CHARS`].
fn plain(text: &str) -> String {
    crate::transcript::clean(text)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MOST_CHARS)
        .collect()
}

/// What goes to the terminal for `text` the `way` says — inside tmux's passthrough where Glut runs in one — with the
/// bell after it always.
pub fn sequence(way: Way, text: &str, tmux: bool) -> String {
    let text = plain(text);
    let own = match way {
        Way::Osc9 => format!("\x1b]9;Glut · {text}\x07"),
        Way::Osc99 => {
            format!("\x1b]99;i=glut:d=0;Glut\x1b\\\x1b]99;i=glut:d=1:p=body;{text}\x1b\\")
        }
        Way::Bell | Way::System => String::new(),
    };
    let own = if tmux && !own.is_empty() {
        format!("\x1bPtmux;{}\x1b\\", own.replace('\x1b', "\x1b\x1b"))
    } else {
        own
    };
    format!("{own}\x07")
}

/// Sends `text` the way `way` says; macOS's own notification is posted beside the screen, never waited for.
pub fn send(out: &mut impl Write, way: Way, text: &str, tmux: bool) -> io::Result<()> {
    if way == Way::System {
        let text = plain(text);
        // The text goes in as an argument, never into the script: nothing it holds is AppleScript.
        let spawned = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                "Glut",
                &text,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            std::thread::spawn(move || child.wait());
        }
    }
    out.write_all(sequence(way, text, tmux).as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(program: &str) -> Env {
        Env {
            term_program: Some(program.into()),
            ..Env::default()
        }
    }

    #[test]
    fn the_way_follows_the_terminal_and_the_system_only_where_asked_and_never_over_ssh() {
        assert_eq!(env("ghostty").way(NotifyWhen::On), Way::Osc9);
        assert_eq!(env("iTerm.app").way(NotifyWhen::On), Way::Osc9);
        assert_eq!(env("WezTerm").way(NotifyWhen::On), Way::Osc9);
        let kitty = Env {
            term: Some("xterm-kitty".into()),
            ..Env::default()
        };
        assert_eq!(kitty.way(NotifyWhen::On), Way::Osc99);
        // Apple Terminal: a click on macOS's message would open Script Editor.
        assert_eq!(env("Apple_Terminal").way(NotifyWhen::On), Way::Bell);
        assert!(!env("Apple_Terminal").reports_focus());
        let tmux_in_ghostty = Env {
            term_program: Some("tmux".into()),
            tmux: true,
            ghostty: true,
            ..Env::default()
        };
        assert_eq!(tmux_in_ghostty.way(NotifyWhen::On), Way::Osc9);
        if cfg!(target_os = "macos") {
            assert_eq!(env("Apple_Terminal").way(NotifyWhen::System), Way::System);
        }
        let remote = Env {
            ssh: true,
            ..env("Apple_Terminal")
        };
        assert_eq!(remote.way(NotifyWhen::System), Way::Bell);
    }

    #[test]
    fn only_a_window_elsewhere_is_told_an_end_after_a_while_an_ask_at_once() {
        let on = Choice {
            when: NotifyWhen::On,
            after: 10,
        };
        let ended = Occasion::Ended {
            ran: Duration::ZERO,
            answered: true,
        };
        let asks = Occasion::Asks {
            tool: "bash".into(),
        };
        let short = Duration::from_secs(3);
        let long = Duration::from_secs(12);
        assert!(
            !worth_a_message(on, Some(true), &ended, long),
            "the owner looks"
        );
        assert!(!worth_a_message(on, Some(true), &asks, long));
        assert!(!worth_a_message(on, Some(false), &ended, short));
        assert!(worth_a_message(on, Some(false), &ended, long));
        assert!(
            worth_a_message(on, Some(false), &asks, short),
            "an ask at once"
        );
        // No focus known: the time alone decides, for an ask too.
        assert!(!worth_a_message(on, None, &asks, short));
        assert!(worth_a_message(on, None, &asks, long));
        let asks_only = Choice {
            when: NotifyWhen::Asks,
            ..on
        };
        assert!(!worth_a_message(asks_only, Some(false), &ended, long));
        assert!(worth_a_message(asks_only, Some(false), &asks, short));
        let off = Choice {
            when: NotifyWhen::Off,
            ..on
        };
        assert!(!worth_a_message(off, Some(false), &asks, long));
    }

    #[test]
    fn what_comes_within_five_seconds_is_folded_and_an_ask_never_waits() {
        glut::lang::set_for_thread(Some(glut::lang::Lang::En));
        let mut notifier = Notifier::default();
        let start = Instant::now();
        let ended = Occasion::Ended {
            ran: Duration::from_secs(125),
            answered: true,
        };
        notifier.push(None, &ended);
        assert_eq!(
            notifier.take("Glut", start).as_deref(),
            Some("Glut · done after 2 min")
        );
        notifier.push(Some("pane 2".into()), &ended);
        notifier.push(Some("pane 3".into()), &ended);
        assert_eq!(notifier.take("Glut", start + Duration::from_secs(1)), None);
        assert_eq!(notifier.due(), Some(start + FOLD_FOR));
        let folded = notifier.take("Glut", start + FOLD_FOR).unwrap();
        assert_eq!(
            folded,
            "Glut · pane 2: done after 2 min · pane 3: done after 2 min"
        );
        notifier.push(
            Some("pane 2".into()),
            &Occasion::Asks {
                tool: "mcp__github__push".into(),
            },
        );
        let asked = notifier.take("Glut", start + FOLD_FOR + Duration::from_secs(1));
        assert_eq!(
            asked.as_deref(),
            Some("Glut · pane 2: asks: use an MCP tool")
        );
        for n in 0..5 {
            notifier.push(
                Some(format!("pane {n}")),
                &Occasion::Asks {
                    tool: "bash".into(),
                },
            );
        }
        let many = notifier.take("Glut", start + FOLD_FOR * 3).unwrap();
        assert!(many.ends_with("· +2 more"), "{many}");
        glut::lang::set_for_thread(None);
    }

    #[test]
    fn an_ask_says_its_kind_never_its_target_and_nothing_ends_the_escape() {
        glut::lang::set_for_thread(Some(glut::lang::Lang::En));
        assert_eq!(
            Occasion::Asks {
                tool: "bash".into()
            }
            .said(),
            "asks: run a command"
        );
        assert_eq!(
            Occasion::Asks {
                tool: "network".into()
            }
            .said(),
            "asks: reach the network"
        );
        let sent = sequence(Way::Osc9, "evil\x07\x1b]9;more\nline", false);
        assert_eq!(
            sent.matches('\x07').count(),
            2,
            "only the escape's end and the bell: {sent:?}"
        );
        assert_eq!(sent.matches('\x1b').count(), 1, "{sent:?}");
        let wrapped = sequence(Way::Osc9, "done", true);
        assert!(wrapped.starts_with("\x1bPtmux;\x1b\x1b]9;"), "{wrapped:?}");
        assert_eq!(sequence(Way::Bell, "done", true), "\x07");
        glut::lang::set_for_thread(None);
    }
}
