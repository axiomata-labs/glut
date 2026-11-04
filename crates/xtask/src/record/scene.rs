//! A scene's script: what the recorder types into Glut and what it waits for, one step per line.
//!
//! ```text
//! # a comment
//! size 120 34                  the terminal, columns and rows
//! project prices               site/demo/prices, copied fresh to ~/glut-demo/prices and committed
//! home fresh                   Glut's own folder new for the take: the owner's providers and keys, nothing else
//! allow "Bash(python3 *)"      a rule of that fresh home's settings, so the scene is not held by a question
//! on "Glut asks" key y         whenever the text comes up: the keys, after a reader's pause (once per appearance)
//! run glut --lang en           the program and its arguments, started in the project
//! wait 20 "build"              until the screen shows the text, at most so many seconds
//! wait 300 "passed" x6         the same, its time played six times faster
//! wait-gone 20 "⏎ send"       until the text is no longer on the screen (Glut started working)
//! settle 2                     until the screen has not changed for so many seconds (at most a minute)
//! pause 1.5                    nothing for so long
//! type "fix the tests"         typed at a person's pace
//! key enter                    keys: enter, esc, tab, backtab, up, down, left, right, space, ctrl-<letter>
//! end                          the recording ends; the program is stopped
//! ```

use std::time::Duration;

/// One step of a scene.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Until the screen shows the text, at most so long; its time divided by the factor in the recording.
    Wait {
        /// What the screen must show.
        text: String,
        /// The longest wait.
        limit: Duration,
        /// How many times faster this stretch plays (1 for real time).
        faster: f64,
    },
    /// Until the text is gone from the screen, at most so long.
    WaitGone {
        text: String,
        limit: Duration,
    },
    /// Until the screen has not changed for so long.
    Settle(Duration),
    /// Nothing for so long.
    Pause(Duration),
    Type(String),
    /// The bytes the keys send.
    Keys(Vec<u8>),
    End,
}

/// A scene: the terminal, the project, the program, the steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// The terminal's columns.
    pub cols: u16,
    /// The terminal's rows.
    pub rows: u16,
    /// The demo project under `site/demo/` the take runs in.
    pub project: Option<String>,
    /// Whether Glut gets a fresh home of its own (`GLUT_HOME`): no sessions, projects or settings of the owner's.
    pub fresh_home: bool,
    /// The allow rules of the fresh home's settings.
    pub allow: Vec<String>,
    /// The program and its arguments.
    pub program: Vec<String>,
    /// Keys sent whenever a text comes up on the screen.
    pub triggers: Vec<(String, Vec<u8>)>,
    /// What happens, in order.
    pub steps: Vec<Step>,
}

/// Reads a scene's script.
///
/// # Errors
///
/// The line and what is wrong with it.
pub fn parse(text: &str) -> Result<Scene, String> {
    let mut scene = Scene {
        cols: 120,
        rows: 34,
        project: None,
        fresh_home: false,
        allow: Vec::new(),
        program: Vec::new(),
        triggers: Vec::new(),
        steps: Vec::new(),
    };
    for (at, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fail = |why: &str| format!("line {}: {why}: {line}", at + 1);
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        match word {
            "size" => {
                let numbers: Vec<u16> = rest
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                let [cols @ 20..=400, rows @ 5..=200] = numbers[..] else {
                    return Err(fail("size takes columns (20–400) and rows (5–200)"));
                };
                (scene.cols, scene.rows) = (cols, rows);
            }
            "project"
                if !rest.is_empty()
                    && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
            {
                scene.project = Some(rest.to_owned());
            }
            "home" if rest == "fresh" => scene.fresh_home = true,
            "on" => {
                let (text, after) =
                    quoted(rest).ok_or_else(|| fail("the text goes in double quotes"))?;
                let names = after
                    .trim()
                    .strip_prefix("key ")
                    .ok_or_else(|| fail("on \"text\" key <keys>"))?;
                let mut bytes = Vec::new();
                for name in names.split_whitespace() {
                    bytes.extend(
                        key(name)
                            .or_else(|| letter(name))
                            .ok_or_else(|| fail(&format!("no key {name}")))?,
                    );
                }
                if text.is_empty() || bytes.is_empty() {
                    return Err(fail("on needs a text and keys"));
                }
                scene.triggers.push((text, bytes));
            }
            "allow" => {
                let (rule, after) =
                    quoted(rest).ok_or_else(|| fail("the rule goes in double quotes"))?;
                if !after.trim().is_empty() || rule.is_empty() {
                    return Err(fail("one rule per line"));
                }
                scene.allow.push(rule);
            }
            "run" if !rest.is_empty() => {
                scene.program = rest.split_whitespace().map(str::to_owned).collect()
            }
            "wait" => {
                let (seconds, rest) = rest
                    .split_once(' ')
                    .ok_or_else(|| fail("wait takes seconds and a text"))?;
                let (text, after) =
                    quoted(rest.trim()).ok_or_else(|| fail("the text goes in double quotes"))?;
                let faster = match after.trim() {
                    "" => 1.0,
                    factor => factor
                        .strip_prefix('x')
                        .and_then(|n| n.parse::<f64>().ok())
                        .filter(|n| *n >= 1.0)
                        .ok_or_else(|| fail("a speed is written x2, x6 …"))?,
                };
                scene.steps.push(Step::Wait {
                    text,
                    limit: seconds_of(seconds).ok_or_else(|| fail("not a number of seconds"))?,
                    faster,
                });
            }
            "wait-gone" => {
                let (seconds, rest) = rest
                    .split_once(' ')
                    .ok_or_else(|| fail("wait-gone takes seconds and a text"))?;
                let (text, after) =
                    quoted(rest.trim()).ok_or_else(|| fail("the text goes in double quotes"))?;
                if !after.trim().is_empty() {
                    return Err(fail("nothing after the text"));
                }
                scene.steps.push(Step::WaitGone {
                    text,
                    limit: seconds_of(seconds).ok_or_else(|| fail("not a number of seconds"))?,
                });
            }
            "settle" => scene.steps.push(Step::Settle(
                seconds_of(rest).ok_or_else(|| fail("seconds"))?,
            )),
            "pause" => scene.steps.push(Step::Pause(
                seconds_of(rest).ok_or_else(|| fail("seconds"))?,
            )),
            "type" => {
                let (text, after) =
                    quoted(rest).ok_or_else(|| fail("the text goes in double quotes"))?;
                if !after.trim().is_empty() {
                    return Err(fail("nothing after the text"));
                }
                scene.steps.push(Step::Type(text));
            }
            "key" => {
                let mut bytes = Vec::new();
                for name in rest.split_whitespace() {
                    bytes.extend(key(name).ok_or_else(|| fail(&format!("no key {name}")))?);
                }
                if bytes.is_empty() {
                    return Err(fail("key names a key"));
                }
                scene.steps.push(Step::Keys(bytes));
            }
            "end" => scene.steps.push(Step::End),
            _ => return Err(fail("not a step")),
        }
    }
    if !scene.allow.is_empty() && !scene.fresh_home {
        return Err(
            "an `allow` rule needs `home fresh`: the owner's own settings are never written"
                .to_owned(),
        );
    }
    // A take never runs in the repository itself or in the owner's own Glut home: their sessions, projects and
    // settings would be on the screen, and the scene could change real files.
    if scene.project.is_none() || !scene.fresh_home {
        return Err("a scene needs `project <name>` and `home fresh`".to_owned());
    }
    if scene.program.is_empty() {
        return Err("a scene needs a `run` line".to_owned());
    }
    Ok(scene)
}

/// A double-quoted text at the start of `rest` (`\"` and `\\` inside), and what follows it.
fn quoted(rest: &str) -> Option<(String, &str)> {
    let inner = rest.strip_prefix('"')?;
    let mut text = String::new();
    let mut chars = inner.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((text, &inner[at + 1..])),
            '\\' => text.push(chars.next()?.1),
            c => text.push(c),
        }
    }
    None
}

fn seconds_of(text: &str) -> Option<Duration> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0 && *seconds < 3600.0)
        .map(Duration::from_secs_f64)
}

/// A single letter or digit, as typed (`y`, `1`).
fn letter(name: &str) -> Option<Vec<u8>> {
    let [c] = name.as_bytes() else { return None };
    c.is_ascii_alphanumeric().then(|| vec![*c])
}

/// The bytes a key sends in a terminal of xterm's kind.
fn key(name: &str) -> Option<Vec<u8>> {
    let bytes: &[u8] = match name {
        "enter" => b"\r",
        "esc" => b"\x1b",
        "tab" => b"\t",
        "backtab" => b"\x1b[Z",
        "space" => b" ",
        "backspace" => b"\x7f",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        _ => {
            let letter = name.strip_prefix("ctrl-")?;
            let [c] = letter.as_bytes() else { return None };
            return c.is_ascii_lowercase().then(|| vec![c - b'a' + 1]);
        }
    };
    Some(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_reads_line_by_line() {
        let scene = parse(
            "# first\nsize 100 30\nproject prices\nhome fresh\nrun glut --lang en\nwait 20 \"say \\\"hi\\\"\"\n\
             wait 300 \"passed\" x6\nsettle 2\npause 1.5\ntype \"fix it\"\nkey ctrl-c enter\nend\n",
        )
        .unwrap();
        assert_eq!((scene.cols, scene.rows), (100, 30));
        assert_eq!(scene.project.as_deref(), Some("prices"));
        assert_eq!(scene.program, ["glut", "--lang", "en"]);
        assert_eq!(
            scene.steps[0],
            Step::Wait {
                text: "say \"hi\"".to_owned(),
                limit: Duration::from_secs(20),
                faster: 1.0
            }
        );
        assert!(matches!(scene.steps[1], Step::Wait { faster, .. } if faster == 6.0));
        assert_eq!(scene.steps[5], Step::Keys(vec![3, b'\r']));
        assert_eq!(scene.steps[6], Step::End);
    }

    #[test]
    fn a_wrong_line_says_where() {
        assert!(
            parse("run glut\nkey ctrl-1\n")
                .unwrap_err()
                .starts_with("line 2")
        );
        assert!(parse("run glut\nwait 5 passed\n").is_err());
        assert!(parse("run glut\nwait 5 \"x\" x0.5\n").is_err());
        assert!(parse("project ../up\nrun glut\n").is_err());
        assert!(parse("pause 1\n").is_err(), "no program");
        assert!(
            parse("run glut\n").is_err(),
            "no demo project nor fresh home"
        );
        assert!(parse("project p\nrun glut\n").is_err(), "no fresh home");
    }
}
