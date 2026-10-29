//! The live line of a turn (T75): above the input while Glut works — how long the turn runs, the tokens come down so
//! far, how fast, what it does right now, the effort and how to stop it (`⠹ 2m 23s · ↓ 8.3k tokens · ~45 tok/s ·
//! running: cargo test · effort medium · ⌃C stops`). Where it is narrow the stop hint goes first, then the effort, the
//! speed and the tokens; the time and what it does stay.

use std::time::Duration;

use glut::Event;
use serde_json::Value;
use unicode_width::UnicodeWidthStr as _;

/// The characters a token holds, roughly: the tokens of an answer still streaming are guessed by its characters until
/// the provider counts them.
const CHARS_PER_TOKEN: usize = 4;
/// The spinner's frames while the screen moves; held still, [`STILL`] stands.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const STILL: char = '•';
/// The longest a command or a path is shown before it is cut.
const TARGET_CHARS: usize = 60;

/// What the turn does right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Doing {
    /// The model thinks, or the next call waits for its first word.
    #[default]
    Thinking,
    /// The model writes its answer.
    Writing,
    /// A command runs.
    Running(String),
    /// A file is read.
    Reading(String),
    /// Another tool, and what it works on.
    Tool(String, String),
    /// A subagent works.
    Agent(String),
}

/// The turn under way, as its events tell it.
#[derive(Debug, Default)]
pub struct Live {
    pub doing: Doing,
    /// The output tokens the provider counted for the turn's calls, its subagents' included.
    counted: u64,
    /// The characters streamed since the last count: guessed tokens until the call's count replaces them.
    streamed: usize,
    /// The tool calls asked and not answered yet: a batch is "thinking" again only once all have answered.
    open: Vec<String>,
    /// A summary is made beside the turn, ahead of the compaction threshold (T63): "📝" beside what it does.
    pub preparing: bool,
}

impl Live {
    /// Takes one of the turn's events.
    pub fn take(&mut self, event: &Event) {
        match event {
            Event::Started { .. } => *self = Live::default(),
            Event::TextDelta { text, .. } => {
                self.doing = Doing::Writing;
                self.streamed += text.chars().count();
            }
            Event::ReasoningDelta { text, .. } => {
                self.doing = Doing::Thinking;
                self.streamed += text.chars().count();
            }
            Event::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                self.open.push(id.clone());
                self.doing = doing_of(name, arguments);
            }
            // The tools answered: the model reads them and thinks again.
            Event::ToolResult { id, .. } => {
                self.open.retain(|open| open != id);
                if self.open.is_empty() {
                    self.doing = Doing::Thinking;
                }
            }
            // A provider that counts nothing (`0`) leaves the guess standing.
            Event::Usage { output_tokens, .. } if *output_tokens > 0 => {
                self.counted += output_tokens;
                self.streamed = 0;
            }
            Event::Preparing { .. } => self.preparing = true,
            // Put in, or the run ended without it.
            Event::Compacted { .. } | Event::Finished { .. } => self.preparing = false,
            Event::Sub { event, .. } => self.take_sub(event),
            _ => {}
        }
    }

    /// A subagent's event: its tokens count, what it does stays its name.
    fn take_sub(&mut self, event: &Event) {
        match event {
            Event::Usage { output_tokens, .. } => self.counted += output_tokens,
            Event::Sub { event, .. } => self.take_sub(event),
            _ => {}
        }
    }

    /// The turn's tokens so far: counted, and guessed for what streams now.
    pub fn tokens(&self) -> u64 {
        self.counted + (self.streamed / CHARS_PER_TOKEN) as u64
    }
}

/// What a tool call does, by its name and arguments.
fn doing_of(name: &str, arguments: &str) -> Doing {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let text = |key: &str| {
        let raw = args.get(key).and_then(Value::as_str).unwrap_or_default();
        let first = raw.lines().next().unwrap_or_default();
        crate::transcript::cut(&crate::transcript::clean(first), TARGET_CHARS)
    };
    match name {
        "bash" => Doing::Running(text("command")),
        "read_file" => Doing::Reading(text("path")),
        "task" => Doing::Agent(text("description")),
        _ => {
            let target = ["path", "pattern", "url", "query", "name"]
                .iter()
                .map(|key| text(key))
                .find(|target| !target.is_empty())
                .unwrap_or_default();
            let name = crate::transcript::clean(name.lines().next().unwrap_or_default());
            Doing::Tool(crate::transcript::cut(&name, TARGET_CHARS), target)
        }
    }
}

/// A turn's time as a person reads it: `45s`, `2m 23s`, `1h 02m`.
pub fn elapsed(time: Duration) -> String {
    let seconds = time.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// What the line says about the turn, before it is fitted to its width.
pub struct Said {
    pub time: Duration,
    pub tokens: u64,
    pub rate: Option<f64>,
    pub effort: Option<String>,
    /// The turn waits for the owner's answer to a question.
    pub asking: bool,
    /// The spinner's frame; `None` held still.
    pub frame: Option<usize>,
}

/// The line, no wider than `width` cells: the pieces that matter least go first.
pub fn line(live: &Live, said: &Said, width: usize) -> String {
    let mark = match said.frame {
        Some(frame) => SPINNER[frame % SPINNER.len()],
        None => STILL,
    };
    let doing = if said.asking {
        tr!(live_asking).to_owned()
    } else {
        match &live.doing {
            Doing::Thinking => tr!(live_thinking).to_owned(),
            Doing::Writing => tr!(live_writing).to_owned(),
            Doing::Running(what) => tr!(live_running, what = what),
            Doing::Reading(what) => tr!(live_reading, what = what),
            Doing::Tool(tool, what) if what.is_empty() => tool.clone(),
            Doing::Tool(tool, what) => format!("{tool}: {what}"),
            Doing::Agent(name) => format!("⬡ {name}"),
        }
    };
    let doing = if live.preparing {
        format!("{doing} 📝")
    } else {
        doing
    };
    let time = format!("{mark} {}", elapsed(said.time));
    let tokens = (said.tokens > 0).then(|| tr!(live_tokens, n = crate::meter::tokens(said.tokens)));
    let rate = said.rate.map(|rate| format!("~{rate:.0} tok/s"));
    let effort = said
        .effort
        .as_ref()
        .map(|effort| tr!(meter_effort, effort = effort));
    let stop = tr!(live_stop).to_owned();
    // `dropped`: how many pieces went, in the order they go — the stop hint, the effort, the speed, the tokens.
    for dropped in 0..=4 {
        let mut pieces = vec![time.clone()];
        pieces.extend(tokens.clone().filter(|_| dropped < 4));
        pieces.extend(rate.clone().filter(|_| dropped < 3));
        pieces.push(doing.clone());
        pieces.extend(effort.clone().filter(|_| dropped < 2));
        if dropped < 1 {
            pieces.push(stop.clone());
        }
        let text = pieces.join(" · ");
        if text.width() <= width {
            return text;
        }
    }
    crate::transcript::cut(&format!("{time} · {doing}"), width)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, arguments: &str) -> Event {
        Event::ToolCall {
            turn: 1,
            id: "c".into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    #[test]
    fn the_turns_events_say_what_it_does_and_its_tokens_counted_then_guessed() {
        let mut live = Live::default();
        live.take(&Event::ReasoningDelta {
            turn: 1,
            text: "x".repeat(40),
        });
        assert_eq!((live.doing.clone(), live.tokens()), (Doing::Thinking, 10));
        live.take(&Event::Usage {
            turn: 1,
            input_tokens: 100,
            cached_input_tokens: 0,
            output_tokens: 12,
        });
        assert_eq!(live.tokens(), 12, "the provider's count replaces the guess");
        live.take(&call("bash", r#"{"command": "cargo test\nand more"}"#));
        assert_eq!(live.doing, Doing::Running("cargo test".into()));
        live.take(&call("read_file", r#"{"path": "src/main.rs"}"#));
        assert_eq!(live.doing, Doing::Reading("src/main.rs".into()));
        live.take(&call("glob", r#"{"pattern": "**/*.rs"}"#));
        assert_eq!(live.doing, Doing::Tool("glob".into(), "**/*.rs".into()));
        live.take(&call("task", r#"{"description": "review", "prompt": "…"}"#));
        assert_eq!(live.doing, Doing::Agent("review".into()));
        live.take(&Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(Event::Usage {
                turn: 1,
                input_tokens: 5,
                cached_input_tokens: 0,
                output_tokens: 30,
            }),
        });
        assert_eq!(live.tokens(), 42, "a subagent's tokens count");
        assert_eq!(live.doing, Doing::Agent("review".into()));
        live.take(&Event::TextDelta {
            turn: 2,
            text: "ok".into(),
        });
        assert_eq!(live.doing, Doing::Writing);
    }

    #[test]
    fn a_narrow_line_drops_the_hint_the_effort_the_speed_and_the_tokens_in_turn() {
        let live = Live {
            doing: Doing::Running("cargo test".into()),
            ..Live::default()
        };
        let said = Said {
            time: Duration::from_secs(143),
            tokens: 8_300,
            rate: Some(45.0),
            effort: Some("medium".into()),
            asking: false,
            frame: None,
        };
        let wide = line(&live, &said, 200);
        assert_eq!(
            wide,
            "• 2m 23s · ↓ 8.3k tokens · ~45 tok/s · running: cargo test · effort medium · ⌃C stops"
        );
        let narrow = line(&live, &said, 60);
        assert!(
            !narrow.contains("⌃C") && !narrow.contains("effort"),
            "{narrow}"
        );
        assert!(
            narrow.contains("2m 23s") && narrow.contains("cargo test"),
            "{narrow}"
        );
        let tiny = line(&live, &said, 30);
        assert!(tiny.starts_with("• 2m 23s"), "{tiny}");
        let asking = line(
            &live,
            &Said {
                asking: true,
                ..said
            },
            200,
        );
        assert!(asking.contains("waits for you"), "{asking}");
    }

    #[test]
    fn a_turns_time_reads_in_seconds_minutes_and_hours() {
        assert_eq!(elapsed(Duration::from_secs(45)), "45s");
        assert_eq!(elapsed(Duration::from_secs(143)), "2m 23s");
        assert_eq!(elapsed(Duration::from_secs(3720)), "1h 02m");
    }

    #[test]
    fn a_batch_thinks_again_once_every_call_answered_and_a_count_of_nothing_keeps_the_guess() {
        let mut live = Live::default();
        live.take(&call("bash", r#"{"command": "a"}"#));
        live.take(&Event::ToolCall {
            turn: 1,
            id: "d".into(),
            name: "bash".into(),
            arguments: r#"{"command": "b"}"#.into(),
        });
        live.take(&Event::ToolResult {
            turn: 1,
            id: "c".into(),
            name: "bash".into(),
            ok: true,
            output: String::new(),
            truncated: false,
        });
        assert_eq!(
            live.doing,
            Doing::Running("b".into()),
            "one call still runs"
        );
        live.take(&Event::ToolResult {
            turn: 1,
            id: "d".into(),
            name: "bash".into(),
            ok: true,
            output: String::new(),
            truncated: false,
        });
        assert_eq!(live.doing, Doing::Thinking);
        live.take(&Event::TextDelta {
            turn: 1,
            text: "x".repeat(80),
        });
        live.take(&Event::Usage {
            turn: 1,
            input_tokens: 10,
            cached_input_tokens: 0,
            output_tokens: 0,
        });
        assert_eq!(
            live.tokens(),
            20,
            "a provider that counts nothing leaves the guess"
        );
    }
}
