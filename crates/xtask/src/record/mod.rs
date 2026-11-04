//! The site's recordings: Glut itself, run in a pseudo-terminal from a scene's script (`site/recordings/<name>.scene`),
//! typed into at a person's pace, its screens kept with their times (`site/recordings/<name>.json`) for the site's
//! player. Real runs with real models: every recording is looked at by the owner before it goes on the site.

mod frames;
mod pty;
mod scene;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use frames::Film;
use pty::Terminal;
use scene::Step;

/// The demo projects live here, each copied fresh before its scene: a folder of the owner's that only this tool
/// writes, marked inside its git folder so nothing else is ever emptied.
const DEMO_DIR: &str = "glut-demo";
const MARKER: &str = "glut-demo";

/// At most one screen per this many milliseconds: Glimm's animation alone would otherwise make thousands.
const FRAME_EVERY: Duration = Duration::from_millis(40);

/// How long a reader looks at a question before a trigger answers it.
const READS_FOR: Duration = Duration::from_millis(2500);

/// How many bytes of one read are kept for the next, so a question to the terminal split between them is found.
const CARRY: usize = 16;

/// How long the last screen stays before the player starts again.
const HOLD_MS: u64 = 3000;

/// The colours the recorder's terminal says it has, asked by Glut at its start: the site's own ground and ink, so
/// Glimm's glow runs into the colour the player draws.
const BACKGROUND: &str = "rgb:0c0c/0b0b/0a0a";
const FOREGROUND: &str = "rgb:ecec/e6e6/dcdc";

/// Records the scene `name`.
///
/// # Errors
///
/// The scene cannot be read, the project not made, the program not started, or a `wait` not met in time.
pub fn record(repo: &Path, name: &str) -> Result<(), String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!(
            "a scene's name is letters, digits and dashes: {name}"
        ));
    }
    let script = repo.join("site/recordings").join(format!("{name}.scene"));
    let text = std::fs::read_to_string(&script)
        .map_err(|err| format!("cannot read {}: {err}", script.display()))?;
    let scene = scene::parse(&text).map_err(|err| format!("{}: {err}", script.display()))?;
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_owned())?;
    let folder = match &scene.project {
        Some(project) => fresh_project(repo, Path::new(&home), project, scene.origin)?,
        None => return Err("a scene runs in a demo project, never in the repository".to_owned()),
    };

    let glut_home = if scene.fresh_home {
        Some(fresh_home(Path::new(&home), &scene.allow)?)
    } else {
        None
    };
    let env: Vec<(&str, &Path)> = glut_home
        .iter()
        .map(|path| ("GLUT_HOME", path.as_path()))
        .collect();
    let mut terminal = Terminal::start(&scene.program, &folder, &env, scene.cols, scene.rows)?;
    let mut take = Take {
        parser: frames::emulator(scene.rows, scene.cols),
        film: Film::new(scene.cols, scene.rows, &home),
        clock: 0.0,
        last: Instant::now(),
        kept: None,
        ended: false,
        carry: Vec::new(),
        triggers: scene
            .triggers
            .iter()
            .map(|trigger| Trigger {
                texts: trigger.texts.clone(),
                keys: trigger.keys.clone(),
                reads: trigger.reads.unwrap_or(READS_FOR),
                seen: None,
                answered: false,
            })
            .collect(),
    };
    let mut typing = Pace(0x2545_f491);
    let mut done = Ok(());
    for step in &scene.steps {
        let result = match step {
            Step::Wait {
                text,
                limit,
                faster,
            } => take.wait_for(&mut terminal, text, *limit, *faster),
            Step::WaitGone { text, limit } => take.wait_gone(&mut terminal, text, *limit),
            Step::Settle(quiet) => take.settle(&mut terminal, *quiet),
            Step::Pause(span) => take.run_for(&mut terminal, *span, 1.0),
            Step::Type(text) => text.chars().try_for_each(|c| {
                terminal.send(c.encode_utf8(&mut [0; 4]).as_bytes())?;
                take.run_for(&mut terminal, typing.next(), 1.0)
            }),
            Step::Keys(bytes) => terminal
                .send(bytes)
                .and_then(|()| take.run_for(&mut terminal, Duration::from_millis(120), 1.0)),
            Step::End => break,
        };
        if let Err(err) = result {
            done = Err(err);
            break;
        }
    }
    // The program and all it started are stopped however the scene ended; what it showed last is kept.
    terminal.end();
    take.keep_now(true);
    // The fresh home's copy of the keys goes at once: no take leaves one behind.
    let secrets = glut_home
        .as_deref()
        .map(take_out_keys)
        .transpose()?
        .unwrap_or_default();
    done?;

    let out = repo.join("site/recordings").join(format!("{name}.json"));
    let json = take.film.json(HOLD_MS);
    let text = serde_json::to_string(&json).map_err(|err| err.to_string())?;
    // A key shown on the screen would otherwise go into the repository with the recording.
    if secrets.iter().any(|secret| text.contains(secret.as_str())) {
        return Err("the recording shows a key of the owner's: not written".to_owned());
    }
    std::fs::write(&out, &text).map_err(|err| format!("cannot write {}: {err}", out.display()))?;
    let frames = json["frames"].as_array().map_or(0, Vec::len);
    println!(
        "record: {} — {frames} screens, {:.1} s, {} KiB",
        out.display(),
        take.clock / 1000.0,
        text.len() / 1024
    );
    Ok(())
}

/// The recording under way: the terminal's screen, the film, and the recording's own clock in milliseconds, which
/// runs slower than the real one inside a `wait … xN`.
struct Take {
    parser: vt100::Parser,
    film: Film,
    clock: f64,
    last: Instant,
    kept: Option<Instant>,
    ended: bool,
    /// The end of the last read, searched again with the next.
    carry: Vec<u8>,
    triggers: Vec<Trigger>,
}

/// Keys sent when a text comes up: once per time it appears, after [`READS_FOR`].
struct Trigger {
    /// Every text that must be on the screen.
    texts: Vec<String>,
    keys: Vec<u8>,
    /// How long the question is read before the keys go.
    reads: Duration,
    /// When the text came up this time.
    seen: Option<Instant>,
    answered: bool,
}

impl Take {
    /// Reads what the program writes for `span`, the clock running `faster` times slower than the real one.
    fn run_for(
        &mut self,
        terminal: &mut Terminal,
        span: Duration,
        faster: f64,
    ) -> Result<(), String> {
        let until = Instant::now() + span;
        while Instant::now() < until {
            self.pump(terminal, until, faster)?;
        }
        Ok(())
    }

    /// Until the screen shows `text`, at most `limit`.
    fn wait_for(
        &mut self,
        terminal: &mut Terminal,
        text: &str,
        limit: Duration,
        faster: f64,
    ) -> Result<(), String> {
        let until = Instant::now() + limit;
        while !Film::text(self.parser.screen()).contains(text) {
            if Instant::now() >= until {
                return Err(format!(
                    "waited {} s for {text:?}; the screen shows:\n{}",
                    limit.as_secs(),
                    Film::text(self.parser.screen())
                ));
            }
            self.pump(
                terminal,
                (Instant::now() + Duration::from_millis(100)).min(until),
                faster,
            )?;
        }
        Ok(())
    }

    /// Until `text` is no longer on the screen, at most `limit`.
    fn wait_gone(
        &mut self,
        terminal: &mut Terminal,
        text: &str,
        limit: Duration,
    ) -> Result<(), String> {
        let until = Instant::now() + limit;
        while Film::text(self.parser.screen()).contains(text) {
            if Instant::now() >= until {
                return Err(format!(
                    "{text:?} stayed on the screen for {} s",
                    limit.as_secs()
                ));
            }
            self.pump(
                terminal,
                (Instant::now() + Duration::from_millis(100)).min(until),
                1.0,
            )?;
        }
        Ok(())
    }

    /// Until no screen was kept for `quiet`, at most a minute.
    fn settle(&mut self, terminal: &mut Terminal, quiet: Duration) -> Result<(), String> {
        let until = Instant::now() + Duration::from_secs(60);
        let started = Instant::now();
        while Instant::now() < until {
            let calm_since = self.kept.unwrap_or(started).max(started);
            if calm_since.elapsed() >= quiet {
                return Ok(());
            }
            self.pump(terminal, Instant::now() + Duration::from_millis(100), 1.0)?;
        }
        Ok(())
    }

    /// Takes what the program wrote until `until`: answers its questions to the terminal, keeps a screen when one
    /// changed and enough time passed since the last.
    fn pump(&mut self, terminal: &mut Terminal, until: Instant, faster: f64) -> Result<(), String> {
        let wait = until.saturating_duration_since(Instant::now());
        match terminal.output.recv_timeout(wait) {
            Ok(bytes) => {
                // A question split over two reads is found whole: the end of the last read goes in front.
                let mut seen = std::mem::take(&mut self.carry);
                let from = seen.len();
                seen.extend_from_slice(&bytes);
                for answer in self.feed(&seen, from) {
                    terminal.send(&answer)?;
                }
                self.carry = seen[seen.len().saturating_sub(CARRY)..].to_vec();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if !self.ended {
                    self.ended = true;
                }
                std::thread::sleep(wait);
            }
        }
        let now = Instant::now();
        self.clock += now.duration_since(self.last).as_secs_f64() * 1000.0 / faster;
        self.last = now;
        self.keep_now(false);
        let shown = Film::text(self.parser.screen());
        // The first trigger in the script's order answers; the others then wait as if they had just seen their text,
        // so one question never gets two answers (`on "git push" key n` before `on "Glut asks" key y`).
        let mut answered = false;
        for trigger in &mut self.triggers {
            if !trigger
                .texts
                .iter()
                .all(|text| shown.contains(text.as_str()))
            {
                (trigger.seen, trigger.answered) = (None, false);
                continue;
            }
            let seen = *trigger.seen.get_or_insert(now);
            if !answered && !trigger.answered && seen.elapsed() >= trigger.reads {
                trigger.answered = true;
                answered = true;
                terminal.send(&trigger.keys)?;
            }
        }
        if answered {
            for trigger in self.triggers.iter_mut().filter(|trigger| !trigger.answered) {
                trigger.seen = trigger.seen.map(|_| now);
            }
        }
        if self.ended {
            return Err(format!(
                "the program ended before the scene did; its last screen:\n{}",
                Film::text(self.parser.screen())
            ));
        }
        Ok(())
    }

    /// Hands the emulator `bytes` from `from` on (what lies before is the last read's end, already handed over), and
    /// answers each question to the terminal as it comes: the cursor's place is the one when it was asked.
    fn feed(&mut self, bytes: &[u8], from: usize) -> Vec<Vec<u8>> {
        let mut said = Vec::new();
        let mut done = from;
        for (at, question) in questions(bytes) {
            let end = at + question.len();
            if end <= from {
                continue;
            }
            self.parser.process(&bytes[done.max(from)..end.max(done)]);
            done = end.max(done);
            said.push(answer(question, self.parser.screen()));
        }
        if done < bytes.len() {
            self.parser.process(&bytes[done.max(from)..]);
        }
        said
    }

    /// Keeps the screen as it is now, unless one was kept less than [`FRAME_EVERY`] ago (`always` keeps it anyway).
    fn keep_now(&mut self, always: bool) {
        let soon = self.kept.is_some_and(|kept| kept.elapsed() < FRAME_EVERY);
        if (always || !soon) && self.film.keep(self.clock as u64, self.parser.screen()) {
            self.kept = Some(Instant::now());
        }
    }
}

/// The questions to the terminal Glut asks, each with its place in `bytes`, in order: the background and foreground
/// colour, its kind (DA1) and the cursor's place. The kitty keyboard question gets no answer, as from a terminal
/// without it.
const QUESTIONS: [&str; 5] = ["\x1b]11;?", "\x1b]10;?", "\x1b[c", "\x1b[0c", "\x1b[6n"];

fn questions(bytes: &[u8]) -> Vec<(usize, &'static str)> {
    let mut found: Vec<(usize, &'static str)> = QUESTIONS
        .iter()
        .flat_map(|question| {
            bytes
                .windows(question.len())
                .enumerate()
                .filter(move |(_, window)| *window == question.as_bytes())
                .map(move |(at, _)| (at, *question))
        })
        .collect();
    found.sort_by_key(|(at, _)| *at);
    found
}

/// What the terminal says to `question`, the screen as it stands when it was asked.
fn answer(question: &str, screen: &vt100::Screen) -> Vec<u8> {
    match question {
        "\x1b]11;?" => format!("\x1b]11;{BACKGROUND}\x1b\\").into_bytes(),
        "\x1b]10;?" => format!("\x1b]10;{FOREGROUND}\x1b\\").into_bytes(),
        "\x1b[6n" => {
            let (row, col) = screen.cursor_position();
            format!("\x1b[{};{}R", row + 1, col + 1).into_bytes()
        }
        _ => b"\x1b[?62;22c".to_vec(),
    }
}

/// The pause after each typed character: 35 to 95 ms, varied the same way in every take.
struct Pace(u32);

impl Pace {
    fn next(&mut self) -> Duration {
        // xorshift: enough to look typed, and the same in every take.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        Duration::from_millis(35 + u64::from(self.0 % 61))
    }
}

/// What of the owner's `~/.glut` a fresh home takes: the providers, their keys and the model lists — the keys only
/// copied from file to file, never read here.
const HOME_TAKES: [&str; 3] = ["providers.json", "credentials.json", "models"];

/// A new `~/glut-demo/.home` for Glut (`GLUT_HOME`), the old one gone first (only one this tool made): the owner's
/// providers and keys, and none of their sessions, projects, settings or snapshots — nothing private on screen.
fn fresh_home(home: &Path, allow: &[String]) -> Result<PathBuf, String> {
    let fresh = home.join(DEMO_DIR).join(".home");
    if fresh.exists() {
        if !fresh.join(MARKER).is_file() {
            return Err(format!(
                "{} is not a home this tool made: not removed",
                fresh.display()
            ));
        }
        std::fs::remove_dir_all(&fresh)
            .map_err(|err| format!("cannot remove {}: {err}", fresh.display()))?;
    }
    let glut = fresh.join(".glut");
    std::fs::create_dir_all(&glut)
        .map_err(|err| format!("cannot make {}: {err}", glut.display()))?;
    std::fs::write(fresh.join(MARKER), "made by cargo run -p xtask -- record\n")
        .map_err(|err| format!("cannot mark {}: {err}", fresh.display()))?;
    for name in HOME_TAKES {
        let source = home.join(".glut").join(name);
        if source.is_dir() {
            copy(&source, &glut.join(name))?;
        } else if source.is_file() {
            // `fs::copy` keeps the file's mode: the keys stay readable by the owner alone.
            std::fs::copy(&source, glut.join(name))
                .map_err(|err| format!("cannot copy {name}: {err}"))?;
        }
    }
    if !allow.is_empty() {
        let settings = serde_json::json!({ "permissions": { "allow": allow } });
        std::fs::write(glut.join("settings.json"), settings.to_string())
            .map_err(|err| format!("cannot write the take's settings: {err}"))?;
    }
    Ok(fresh)
}

/// Removes the fresh home's copy of the keys after a take, and returns every value it held, to make sure none of them
/// is in the recording (they are compared, never shown).
fn take_out_keys(fresh: &Path) -> Result<Vec<String>, String> {
    let keys = fresh.join(".glut").join("credentials.json");
    let mut values = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&keys) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            collect_strings(&json, &mut values);
        }
        std::fs::remove_file(&keys)
            .map_err(|err| format!("cannot remove the take's copy of the keys: {err}"))?;
    }
    // Short values (a provider's name, a date) are no secret worth refusing a recording for.
    values.retain(|value| value.len() >= MIN_SECRET);
    Ok(values)
}

/// The shortest value of the keys' file checked against a recording.
const MIN_SECRET: usize = 16;

fn collect_strings(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => into.push(text.clone()),
        serde_json::Value::Array(items) => {
            items.iter().for_each(|item| collect_strings(item, into))
        }
        serde_json::Value::Object(fields) => {
            fields.values().for_each(|item| collect_strings(item, into))
        }
        _ => {}
    }
}

/// `site/demo/<project>` copied to `~/glut-demo/<project>`, the old copy gone first (only one this tool made), and
/// committed, so the scene's diffs show what Glut changed.
fn fresh_project(repo: &Path, home: &Path, project: &str, origin: bool) -> Result<PathBuf, String> {
    let source = repo.join("site/demo").join(project);
    if !source.is_dir() {
        return Err(format!("no demo project {}", source.display()));
    }
    let folder = home.join(DEMO_DIR).join(project);
    // The mark lies beside the folder, so it is there before the first copy or git step can fail, and never in
    // the project the scene shows.
    let mark = home.join(DEMO_DIR).join(format!(".{project}.{MARKER}"));
    if folder.exists() {
        if !mark.is_file() {
            return Err(format!(
                "{} is not a demo folder this tool made: not removed",
                folder.display()
            ));
        }
        std::fs::remove_dir_all(&folder)
            .map_err(|err| format!("cannot remove {}: {err}", folder.display()))?;
    }
    std::fs::create_dir_all(home.join(DEMO_DIR))
        .map_err(|err| format!("cannot make the demo folder: {err}"))?;
    std::fs::write(&mark, "made by cargo run -p xtask -- record\n")
        .map_err(|err| format!("cannot mark {}: {err}", folder.display()))?;
    copy(&source, &folder)?;
    // git alone: none of the owner's config, hooks, signing or a GIT_DIR that would point elsewhere.
    let git = |args: &[&str]| -> Result<(), String> {
        let status = Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(&folder)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Glut demo")
            .env("GIT_AUTHOR_EMAIL", "demo@glut.invalid")
            .env("GIT_COMMITTER_NAME", "Glut demo")
            .env("GIT_COMMITTER_EMAIL", "demo@glut.invalid")
            .output()
            .map_err(|err| format!("git: {err}"))?;
        if status.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&status.stderr)
            ))
        }
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["add", "-A"])?;
    git(&["commit", "-q", "-m", "The project as it was yesterday"])?;
    if origin {
        // A remote of its own beside the project, outside the folder the take works in.
        let bare = home.join(DEMO_DIR).join(format!("{project}-origin.git"));
        if bare.exists() {
            std::fs::remove_dir_all(&bare)
                .map_err(|err| format!("cannot remove {}: {err}", bare.display()))?;
        }
        let bare_text = bare.to_string_lossy().into_owned();
        git(&["init", "-q", "--bare", &bare_text])?;
        git(&["remote", "add", "origin", &bare_text])?;
        git(&["push", "-q", "-u", "origin", "main"])?;
    }
    Ok(folder)
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|err| format!("cannot make {}: {err}", to.display()))?;
    for entry in
        std::fs::read_dir(from).map_err(|err| format!("cannot list {}: {err}", from.display()))?
    {
        let entry = entry.map_err(|err| err.to_string())?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)
                .map_err(|err| format!("cannot copy {}: {err}", source.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_answers_glut_s_questions_in_their_order() {
        let mut take = Take {
            parser: frames::emulator(2, 10),
            film: Film::new(10, 2, "/nowhere"),
            clock: 0.0,
            last: Instant::now(),
            kept: None,
            ended: false,
            carry: Vec::new(),
            triggers: Vec::new(),
        };
        let said = take.feed(b"\x1b]11;?\x07\x1b[c\x1b[?u", 0);
        assert_eq!(said.len(), 2, "no answer to the kitty question");
        assert!(said[0].starts_with(b"\x1b]11;rgb:"));
        assert_eq!(said[1], b"\x1b[?62;22c");
        // The cursor's place is where it stood when asked, not after what followed.
        let said = take.feed(b"ab\x1b[6ncdef", 0);
        assert_eq!(said, [b"\x1b[1;3R".to_vec()]);
        // A question split between two reads: the carried end and the new bytes, answered once.
        let said = take.feed(b"xx\x1b[6", 0);
        assert!(said.is_empty());
        let said = take.feed(b"xx\x1b[6n", 4);
        assert_eq!(said.len(), 1);
    }

    #[test]
    fn typing_varies_but_stays_human() {
        let mut pace = Pace(1);
        let pauses: Vec<u128> = (0..50).map(|_| pace.next().as_millis()).collect();
        assert!(pauses.iter().all(|ms| (35..=95).contains(ms)));
        assert!(pauses.windows(2).any(|pair| pair[0] != pair[1]));
    }
}
