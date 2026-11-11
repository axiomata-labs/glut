//! The site's recordings: Glut itself, run in a pseudo-terminal from a scene's script (`site/recordings/<name>.scene`),
//! typed into at a person's pace, its screens kept with their times (`site/recordings/<name>.json`) for the site's
//! player. Real runs with real models: every recording is looked at by the owner before it goes on the site.

mod frames;
mod graphics;
mod pty;
mod scene;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use frames::Film;
use graphics::Graphics;
use pty::Terminal;
use scene::Step;

/// The demo projects live here, each copied fresh before its scene: a folder of the owner's that only this tool
/// writes, marked inside its git folder so nothing else is ever emptied.
const DEMO_DIR: &str = "glut-demo";
const MARKER: &str = "glut-demo";

/// At most one screen per this many milliseconds: Glimm's animation alone would otherwise make thousands.
const FRAME_EVERY: Duration = Duration::from_millis(40);

/// How long a reader looks at a question before a trigger answers it: long enough to read what is asked and every
/// choice under it (owner, 2026-11-11: at 2.5 s, shortened with the model's time, a card was gone before it was seen).
const READS_FOR: Duration = Duration::from_millis(4000);

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
    for other in &scene.beside {
        fresh_project(repo, Path::new(&home), other, false)?;
    }

    // The owner's keys and sign-ins as the take is given them, to tell afterwards which it renewed.
    let given = stored_secrets(&Path::new(&home).join(".glut").join(CREDENTIALS));
    let glut_home = if scene.fresh_home {
        let manifest = std::fs::read_to_string(repo.join("crates/glut-tui/Cargo.toml"))
            .map_err(|err| format!("cannot read glut-tui's manifest: {err}"))?;
        let version = crate::site::manifest_version(&manifest)
            .ok_or("crates/glut-tui/Cargo.toml names no version")?;
        let fresh = fresh_home(
            Path::new(&home),
            &scene.allow,
            &scene.settings,
            scene.ghostty,
            &version,
        )?;
        for set in &scene.tasks {
            let source = repo.join("site/demo/arena").join(set);
            if !source.is_dir() {
                return Err(format!("no demo task set {}", source.display()));
            }
            // Only the demo's own tasks: the owner's private sets never come into a take.
            copy(&source, &fresh.join(".glut/arena/tasks/sets").join(set))?;
        }
        if scene.neovim {
            neovim_in(repo, &fresh)?;
        }
        Some(fresh)
    } else {
        None
    };
    let neovim: Vec<(&str, PathBuf)> = match (&glut_home, scene.neovim) {
        (Some(fresh), true) => NEOVIM_FOLDERS
            .iter()
            .map(|(name, below)| (*name, fresh.join(below)))
            .collect(),
        _ => Vec::new(),
    };
    let env: Vec<(&str, &Path)> = glut_home
        .iter()
        .map(|path| ("GLUT_HOME", path.as_path()))
        .chain(neovim.iter().map(|(name, path)| (*name, path.as_path())))
        // Glut draws its images through the terminal where it names itself Ghostty, as the owner's does.
        .chain(
            scene
                .ghostty
                .then_some(("TERM_PROGRAM", Path::new("ghostty"))),
        )
        .collect();
    let mut terminal = Terminal::start(&scene.program, &folder, &env, scene.cols, scene.rows)?;
    let mut take = Take {
        parser: frames::emulator(scene.rows, scene.cols),
        film: Film::new(scene.cols, scene.rows, &home),
        clock: 0.0,
        last: Instant::now(),
        kept: None,
        ended: false,
        asking: false,
        carry: Vec::new(),
        graphics: Graphics::default(),
        images: std::collections::HashMap::new(),
        triggers: scene
            .triggers
            .iter()
            .map(|trigger| Trigger {
                texts: trigger.texts.clone(),
                keys: trigger.keys.clone(),
                reads: trigger.reads.unwrap_or(READS_FOR),
                seen: None,
                answered: false,
                card: None,
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
            Step::WaitGone {
                text,
                limit,
                steady,
            } => take.wait_gone(&mut terminal, text, *limit, *steady),
            Step::Pause(span) => take.run_for(&mut terminal, *span, 1.0),
            Step::Type(text) => text.chars().try_for_each(|c| {
                terminal.send(c.encode_utf8(&mut [0; 4]).as_bytes())?;
                take.run_for(&mut terminal, typing.next(), 1.0)
            }),
            Step::Paste(text) => {
                // A path from the owner's home, written without it: no scene names the owner's folder.
                let text = match text.strip_prefix("~/") {
                    Some(rest) => format!("{home}/{rest}"),
                    None => text.clone(),
                };
                terminal
                    .send(format!("\x1b[200~{text}\x1b[201~").as_bytes())
                    .and_then(|()| take.run_for(&mut terminal, Duration::from_millis(300), 1.0))
            }
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
    // A plan the scene made is the system's job until it is booted out: none outlives its take, however it ended.
    if let Some(fresh) = &glut_home {
        unload_plans(fresh);
    }
    take.keep_now(true);
    // A sign-in the take renewed holds the only good refresh token now: it goes back to the owner before the copy goes.
    if let Some(fresh) = &glut_home {
        for account in give_back_renewals(Path::new(&home), fresh, &given)? {
            eprintln!(
                "record: the take renewed the sign-in {account}; it went back into ~/.glut/credentials.json"
            );
        }
    }
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
    /// Whether a question stands that a trigger has yet to answer: the clock then runs as the real one does.
    asking: bool,
    /// The end of the last read, searched again with the next.
    carry: Vec<u8>,
    /// The pictures the program drew through the terminal, and their images' places in the film by their number.
    graphics: Graphics,
    images: std::collections::HashMap<u32, usize>,
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
    /// The question's card this trigger reads or has answered, as it stood when it came up: another card in its
    /// place, with no screen between that lacks the text, is a new question.
    card: Option<String>,
}

/// The question card whose title holds `text`: the card's own columns, from its corner `┌` to `┐`, and its rows down
/// to its lower edge `└` — a pane beside it (another Glut window in tmux, Glimm moving in it) is no part of the
/// question. `None` where `text` stands in no card: in the transcript, or with no edge under it.
fn card_of(shown: &str, text: &str) -> Option<String> {
    let mut rows = shown.lines().skip_while(|row| !row.contains(text));
    let first = rows.next()?;
    let before = first[..first.find(text)?].chars().count();
    let title: Vec<char> = first.chars().collect();
    let left = title[..before].iter().rposition(|c| *c == '┌')?;
    let right = title[left..]
        .iter()
        .position(|c| *c == '┐')
        .map_or(title.len(), |at| left + at + 1);
    let within = |row: &str| -> String {
        let part: String = row.chars().skip(left).take(right - left).collect();
        part.trim_end().to_owned()
    };
    let mut card = vec![within(first)];
    for row in rows {
        let part = within(row);
        let edge = part.starts_with('└');
        card.push(part);
        if edge {
            return Some(card.join("\n"));
        }
    }
    None
}

/// How many times faster than the real clock the recording's runs: `faster` inside a `wait … xN`, but never while
/// a question stands — the model's time is shortened, the reader's is not.
fn pace(asking: bool, faster: f64) -> f64 {
    if asking { 1.0 } else { faster }
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
        steady: Duration,
    ) -> Result<(), String> {
        let until = Instant::now() + limit;
        // Since when the text has been away; it must stay away `steady` long.
        let mut away: Option<Instant> = None;
        loop {
            let shown = Film::text(self.parser.screen()).contains(text);
            match (shown, away) {
                (true, _) => away = None,
                (false, None) => away = Some(Instant::now()),
                (false, Some(since)) if since.elapsed() >= steady => return Ok(()),
                (false, Some(_)) => {}
            }
            if !shown && steady.is_zero() {
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(format!(
                    "{text:?} stayed on the screen for {} s; the screen shows:\n{}",
                    limit.as_secs(),
                    Film::text(self.parser.screen())
                ));
            }
            self.pump(
                terminal,
                (Instant::now() + Duration::from_millis(100)).min(until),
                1.0,
            )?;
        }
    }

    /// Takes what the program wrote until `until`: answers its questions to the terminal, keeps a screen when one
    /// changed and enough time passed since the last.
    fn pump(&mut self, terminal: &mut Terminal, until: Instant, faster: f64) -> Result<(), String> {
        let wait = until.saturating_duration_since(Instant::now());
        match terminal.output.recv_timeout(wait) {
            Ok(bytes) => {
                let bytes = self.graphics.cut(&bytes);
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
        self.clock +=
            now.duration_since(self.last).as_secs_f64() * 1000.0 / pace(self.asking, faster);
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
                (trigger.seen, trigger.answered, trigger.card) = (None, false, None);
                continue;
            }
            // Another question's card in the place of the one this trigger was reading or had answered: read anew,
            // by every trigger alike, so the first in the script's order still answers it. Where the text stands in
            // no card (left in the transcript once the card closed), nothing is new.
            let card = trigger.texts.first().and_then(|text| card_of(&shown, text));
            if card.is_some() && trigger.card.is_some() && trigger.card != card {
                (trigger.seen, trigger.answered) = (None, false);
            }
            if trigger.seen.is_none() {
                trigger.card.clone_from(&card);
            }
            let seen = *trigger.seen.get_or_insert(now);
            if !answered && !trigger.answered && seen.elapsed() >= trigger.reads {
                trigger.answered = true;
                answered = true;
                terminal.send(&trigger.keys)?;
            }
        }
        self.asking = self
            .triggers
            .iter()
            .any(|trigger| trigger.seen.is_some() && !trigger.answered);
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
            if question == graphics::TOKEN {
                self.draw_picture();
                continue;
            }
            said.push(answer(question, self.parser.screen()));
        }
        if done < bytes.len() {
            self.parser.process(&bytes[done.max(from)..]);
        }
        said
    }

    /// Applies the next graphics command at the cursor: an image sent whole goes into the film, and the pictures
    /// standing are the film's from now on.
    fn draw_picture(&mut self) {
        let (row, col) = self.parser.screen().cursor_position();
        if let Some((number, data)) = self.graphics.apply(row, col) {
            let index = self.film.image(data);
            self.images.insert(number, index);
        }
        let images = &self.images;
        self.film.stand(
            self.graphics
                .standing(|number| images.get(&number).copied()),
        );
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
/// colour, its kind (DA1) and the cursor's place — and where a graphics command stood ([`graphics::TOKEN`]), which
/// gets no answer either (Glut sends them quiet, `q=2`). The kitty keyboard question gets no answer, as from a
/// terminal without it.
const QUESTIONS: [&str; 6] = [
    "\x1b]11;?",
    "\x1b]10;?",
    "\x1b[c",
    "\x1b[0c",
    "\x1b[6n",
    graphics::TOKEN,
];

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

/// The pause after each typed character: 18 to 52 ms, varied the same way in every take — a quick typist's pace
/// (owner, 2026-11-11: at 35 to 95 ms a prompt took longer to type than to read).
struct Pace(u32);

impl Pace {
    fn next(&mut self) -> Duration {
        // xorshift: enough to look typed, and the same in every take.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        Duration::from_millis(18 + u64::from(self.0 % 35))
    }
}

/// What of the owner's `~/.glut` a fresh home takes: the providers, their keys and the model lists — the keys only
/// copied from file to file, never read here.
/// Neovim's folders, each below the fresh home: its config, plugins, history and cache are the take's alone.
const NEOVIM_FOLDERS: [(&str, &str); 4] = [
    ("XDG_CONFIG_HOME", ".config"),
    ("XDG_DATA_HOME", ".local/share"),
    ("XDG_STATE_HOME", ".local/state"),
    ("XDG_CACHE_HOME", ".cache"),
];

/// Neovim for a take, in the fresh home's folders: the demo's `init.lua` and glut.nvim — the checkout beside the
/// repository, its code alone — as a package Neovim loads at start.
fn neovim_in(repo: &Path, fresh: &Path) -> Result<(), String> {
    let config = fresh.join(".config/nvim");
    std::fs::create_dir_all(&config)
        .map_err(|err| format!("cannot make {}: {err}", config.display()))?;
    let init = repo.join("site/demo/nvim/init.lua");
    std::fs::copy(&init, config.join("init.lua"))
        .map_err(|err| format!("cannot copy {}: {err}", init.display()))?;
    let plugin = repo.join("../glut.nvim");
    if !plugin.join("lua/glut").is_dir() {
        return Err(format!("no glut.nvim at {}", plugin.display()));
    }
    let package = fresh.join(".local/share/nvim/site/pack/glut/start/glut.nvim");
    for part in ["lua", "plugin"] {
        copy(&plugin.join(part), &package.join(part))?;
    }
    Ok(())
}

const HOME_TAKES: [&str; 3] = ["providers.json", "credentials.json", "models"];

/// A new `~/glut-demo/.home` for Glut (`GLUT_HOME`), the old one gone first (only one this tool made): the owner's
/// providers and keys, and none of their sessions, projects, settings or snapshots — nothing private on screen.
fn fresh_home(
    home: &Path,
    allow: &[String],
    settings: &serde_json::Map<String, serde_json::Value>,
    ghostty: bool,
    version: &str,
) -> Result<PathBuf, String> {
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
    let mut written = settings.clone();
    if !allow.is_empty() {
        written.insert(
            "permissions".to_owned(),
            serde_json::json!({ "allow": allow }),
        );
    }
    if !written.is_empty() {
        std::fs::write(
            glut.join("settings.json"),
            serde_json::Value::Object(written).to_string(),
        )
        .map_err(|err| format!("cannot write the take's settings: {err}"))?;
    }
    if ghostty {
        // Ghostty draws Powerline's separators itself, the site's font has none: the footer stays as in every scene.
        std::fs::write(glut.join("prefs.json"), r#"{"footer":"plain"}"#)
            .map_err(|err| format!("cannot write the take's prefs: {err}"))?;
        // A kept preference makes this no first start, and Glut would open with what is new since 0.1.0: the take
        // has seen this version's news, so its scene starts as every other does.
        std::fs::write(
            glut.join("version.json"),
            serde_json::json!({ "seen": version }).to_string(),
        )
        .map_err(|err| format!("cannot write the take's seen version: {err}"))?;
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

/// Every LaunchAgent the take's Glut wrote into its fresh home (`schedule::agent_file`) booted out of the owner's
/// launchd, and its file removed. Only Glut's own labels, and only from that home: none of the owner's plans.
fn unload_plans(fresh: &Path) {
    let dir = fresh.join("Library").join("LaunchAgents");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let uid = rustix::process::getuid().as_raw();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(label) = name
            .strip_suffix(".plist")
            .filter(|label| label.starts_with("local.glut.schedule."))
        else {
            continue;
        };
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{label}")])
            .output();
        let _ = std::fs::remove_file(entry.path());
        eprintln!("record: the scene's plan {label} is booted out of launchd");
    }
}

/// The keys' file of a Glut home, below its `.glut`.
const CREDENTIALS: &str = "credentials.json";

/// The `secrets` of a keys' file by account, empty where it cannot be read.
fn stored_secrets(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("secrets")?.as_object().cloned())
        .unwrap_or_default()
}

/// Every account whose entry the take changed (a subscription's renewal replaces its refresh token, which spends the
/// owner's) written back into the owner's keys' file — under the owner's lock of that account, as Glut's renewal
/// takes it, and only where the owner's entry is still the one the take was given: one the owner renewed meanwhile
/// stays. No account is added, none removed.
fn give_back_renewals(
    home: &Path,
    fresh: &Path,
    given: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<String>, String> {
    let taken = stored_secrets(&fresh.join(".glut").join(CREDENTIALS));
    let renewed: Vec<(&String, &serde_json::Value)> = taken
        .iter()
        .filter(|(account, value)| given.get(*account).is_some_and(|before| before != *value))
        .collect();
    let mut back = Vec::new();
    for (account, value) in renewed {
        let _lock = account_lock(home, account)?;
        let path = home.join(".glut").join(CREDENTIALS);
        let text = std::fs::read_to_string(&path)
            .map_err(|err| format!("cannot read the owner's keys again: {err}"))?;
        let mut json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| format!("the owner's keys do not read: {err}"))?;
        let Some(secrets) = json
            .get_mut("secrets")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        if secrets.get(account) != given.get(account) {
            continue;
        }
        secrets.insert(account.clone(), value.clone());
        write_private(&path, &json.to_string())?;
        back.push(account.clone());
    }
    Ok(back)
}

/// Glut's own lock of `account`'s record (`subscription::Lock`: `~/.glut/locks/<account>.lock`), waited for.
fn account_lock(home: &Path, account: &str) -> Result<std::fs::File, String> {
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
    let dir = home.join(".glut").join("locks");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|err| format!("cannot make {}: {err}", dir.display()))?;
    let name: String = account
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "@.-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(dir.join(format!("{name}.lock")))
        .map_err(|err| format!("cannot open the sign-in's lock: {err}"))?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
        .map_err(|err| format!("cannot take the sign-in's lock: {err}"))?;
    Ok(file)
}

/// `text` as the whole of `path`: a file beside it, readable by the owner alone from its first byte, renamed into
/// place.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let beside = path.with_extension(format!("json.{}.new", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&beside)
        .map_err(|err| format!("cannot write the owner's keys: {err}"))?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&beside, path));
    if let Err(err) = written {
        let _ = std::fs::remove_file(&beside);
        return Err(format!("cannot write the owner's keys: {err}"));
    }
    Ok(())
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
    fn a_question_is_its_card_and_stands_at_the_readers_pace() {
        let first = "● working\n┌ Glut asks: mcp__stock__products · build ──┐\n│  stock/products   │\n│ y  allow this \
                     once │\n└──────┘\n›";
        let second = first.replace("products", "stock_level");
        let card = card_of(first, "Glut asks").unwrap();
        assert!(card.starts_with("┌ Glut asks: mcp__stock__products"));
        assert!(
            card.ends_with("└──────┘"),
            "down to the card's edge, no further"
        );
        assert_ne!(
            card_of(&second, "Glut asks"),
            Some(card),
            "the next question right behind it is another card"
        );
        assert_eq!(card_of("nothing here", "Glut asks"), None);
        assert_eq!(
            card_of("a\n● it said: Glut asks: x\n└─┘", "Glut asks"),
            None,
            "the words in the transcript are no card"
        );
        assert_eq!(
            card_of("┌ Glut asks: bash ─┐\n│ make │", "Glut asks"),
            None,
            "no lower edge: not a whole card"
        );
        // Two panes side by side: what moves in the other one is no part of the question.
        let beside = |other: &str| {
            format!(
                "{other} ┃ ┌ Glut asks: bash ─┐\n{other} ┃ │  make test      │\n{other} ┃ └────────────────┘"
            )
        };
        assert_eq!(
            card_of(&beside("▗▄▖"), "Glut asks"),
            card_of(&beside("▟▘▛"), "Glut asks")
        );
        assert!(card_of(&beside("▗▄▖"), "Glut asks").is_some());
        assert_eq!(pace(false, 3.0), 3.0, "the model's time is shortened");
        assert_eq!(pace(true, 3.0), 1.0, "the reader's is not");
    }

    #[test]
    fn a_sign_in_the_take_renewed_goes_back_unless_the_owner_renewed_it_too() {
        let base = std::env::temp_dir().join(format!("xtask-renewals-{}", std::process::id()));
        let (home, fresh) = (base.join("home"), base.join("fresh"));
        for dir in [&home, &fresh] {
            std::fs::create_dir_all(dir.join(".glut")).unwrap();
        }
        let keys = |dir: &Path, json: serde_json::Value| {
            std::fs::write(dir.join(".glut").join(CREDENTIALS), json.to_string()).unwrap()
        };
        keys(
            &home,
            serde_json::json!({"secrets": {"plan@a": "old", "mine@b": "old", "key@c": "same"}}),
        );
        let given = stored_secrets(&home.join(".glut").join(CREDENTIALS));
        // The take renewed two sign-ins and made one up; the owner renewed one of them meanwhile.
        keys(
            &fresh,
            serde_json::json!({"secrets": {"plan@a": "new", "mine@b": "take's", "key@c": "same", "x@d": "1"}}),
        );
        keys(
            &home,
            serde_json::json!({"secrets": {"plan@a": "old", "mine@b": "owner's", "key@c": "same"}}),
        );
        assert_eq!(
            give_back_renewals(&home, &fresh, &given).unwrap(),
            ["plan@a"]
        );
        let now = stored_secrets(&home.join(".glut").join(CREDENTIALS));
        assert_eq!(now["plan@a"], "new");
        assert_eq!(now["mine@b"], "owner's", "the owner's own renewal stays");
        assert!(!now.contains_key("x@d"), "no account is added");
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(home.join(".glut").join(CREDENTIALS))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn the_terminal_answers_glut_s_questions_in_their_order() {
        let mut take = Take {
            parser: frames::emulator(2, 10),
            film: Film::new(10, 2, "/nowhere"),
            clock: 0.0,
            last: Instant::now(),
            kept: None,
            ended: false,
            asking: false,
            carry: Vec::new(),
            graphics: Graphics::default(),
            images: std::collections::HashMap::new(),
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
    fn a_picture_is_placed_where_the_cursor_stood_and_gets_no_answer() {
        let mut take = Take {
            parser: frames::emulator(4, 10),
            film: Film::new(10, 4, "/nowhere"),
            clock: 0.0,
            last: Instant::now(),
            kept: None,
            ended: false,
            asking: false,
            carry: Vec::new(),
            graphics: Graphics::default(),
            images: std::collections::HashMap::new(),
            triggers: Vec::new(),
        };
        let sent = take
            .graphics
            .cut(b"\x1b_Ga=t,f=100,i=1,q=2,m=0;QUJD\x1b\\\x1b7\x1b[3;2H\x1b_Ga=p,i=1,p=1,c=4,r=2,C=1,q=2\x1b\\\x1b8x");
        assert!(take.feed(&sent, 0).is_empty(), "graphics are quiet");
        take.keep_now(true);
        let json = take.film.json(0);
        assert_eq!(json["images"], serde_json::json!(["QUJD"]));
        assert_eq!(
            json["frames"][0]["pics"],
            serde_json::json!([[0, 1, 2, 4, 2]])
        );
        assert!(
            json["frames"][0]["rows"]["0"][0][0]
                .as_str()
                .unwrap()
                .starts_with('x')
        );
    }

    #[test]
    fn typing_varies_but_stays_human() {
        let mut pace = Pace(1);
        let pauses: Vec<u128> = (0..50).map(|_| pace.next().as_millis()).collect();
        assert!(pauses.iter().all(|ms| (18..=52).contains(ms)));
        assert!(pauses.windows(2).any(|pair| pair[0] != pair[1]));
    }
}
