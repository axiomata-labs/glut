//! `glut watch` (`docs/plans/glut-tui.md`, T21, TUI8d): another session followed from its event log, read-only — a
//! Studio card from another terminal, the owner's own session from a second window. The stream is drawn as the
//! session's own screen draws it; nothing typed reaches the session, which goes on whether anyone watches or not.

use std::io::{self, Read as _, Seek as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate};
use glut::Event;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::log::{self, Logged};
use crate::transcript::{Kind, Tone, Transcript};
use crate::{prefs, screen, store, theme};

/// How often the log is looked at for new lines.
const POLL: Duration = Duration::from_millis(250);
/// The most a look reads at once: a long log comes in in parts, and the screen is drawn in between.
const CHUNK: u64 = 1024 * 1024;
/// How many lines a page key scrolls, less than a screen so the reader keeps their place.
const PAGE: usize = 20;
/// The most of the stream the viewer keeps, in blocks: a long session's oldest go, the newest are what one follows.
const KEPT_BLOCKS: usize = 5_000;
/// The longest line the viewer waits for the end of: a file with no line breaks is no log, and must not fill memory.
const MAX_LINE: usize = 4 * 1024 * 1024;

/// The log `what` names: a file, a session's id in `~/.glut/sessions`, or — `None` — the newest session of `cwd`.
///
/// # Errors
///
/// A message when nothing is named and the folder has no session, or an id is no session's.
pub fn resolve(home: &Path, cwd: &Path, what: Option<&str>) -> Result<PathBuf, String> {
    match what {
        Some(name) if Path::new(name).is_file() => Ok(PathBuf::from(name)),
        Some(id)
            if !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
        {
            let path = log::session_log(home, id);
            if path.is_file() || store::Session::load(home, id).is_ok() {
                Ok(path)
            } else {
                Err(tr!(no_session_or_log, name = format!("{id:?}")))
            }
        }
        Some(other) => Err(tr!(no_session_or_log, name = format!("{other:?}"))),
        None => log::newest(home, cwd).ok_or_else(|| tr!(nothing_to_watch).into()),
    }
}

/// What has been read of the log so far.
#[derive(Default)]
struct Followed {
    transcript: Transcript,
    /// What of the log was read, in bytes.
    offset: u64,
    /// The start of a line whose end has not been written yet.
    partial: String,
    /// The session's first message, as its name.
    title: String,
    /// A turn runs: begun, not finished yet.
    working: bool,
    input_tokens: u64,
    output_tokens: u64,
    /// The log is full: nothing more will come.
    full: bool,
}

impl Followed {
    /// `text` read from the log: every whole line taken in, the rest kept for the next look.
    fn take(&mut self, text: &str) {
        self.partial.push_str(text);
        if self.partial.len() > MAX_LINE && !self.partial.contains('\n') {
            self.partial.clear();
        }
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            match log::read(line.trim_end()) {
                Some(Logged::Owner(text)) => {
                    if self.title.is_empty() {
                        self.title = store::title_of(&text);
                    }
                    self.working = true;
                    self.transcript.push(Kind::Owner(text));
                }
                Some(Logged::Event(event)) => self.event(&event),
                Some(Logged::Full) => {
                    self.full = true;
                    self.transcript.note(tr!(log_full_note), Tone::Fault);
                }
                Some(Logged::Head(_)) | None => {}
            }
        }
        let over = self.transcript.blocks.len().saturating_sub(KEPT_BLOCKS);
        if over > 0 {
            self.transcript.blocks.drain(..over);
        }
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                self.input_tokens += input_tokens;
                self.output_tokens += output_tokens;
            }
            Event::Finished { .. } => self.working = false,
            Event::Started { .. } => self.working = true,
            _ => {}
        }
        self.transcript.take(event);
        if matches!(event, Event::Finished { .. }) {
            self.transcript.settle();
        }
    }

    /// The log was cut back or replaced (a new session in a host's file): read again from its start.
    fn restart(&mut self) {
        let thinking = self.transcript.thinking;
        *self = Followed::default();
        self.transcript.thinking = thinking;
    }

    /// Reads the next part of what was added to `path` since the last look, at most [`CHUNK`]; whether more is left.
    fn look(&mut self, path: &Path) -> bool {
        use std::os::unix::fs::OpenOptionsExt as _;
        // Never through a link, as the log is never written through one.
        let Ok(mut file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(path)
        else {
            return false;
        };
        let Ok(len) = file.metadata().map(|meta| meta.len()) else {
            return false;
        };
        if len < self.offset {
            self.restart();
        }
        if len == self.offset || file.seek(io::SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut bytes = Vec::new();
        if file.take(CHUNK).read_to_end(&mut bytes).is_err() || bytes.is_empty() {
            return false;
        }
        // A cut in the middle of a character waits for the rest, as a cut in the middle of a line does; a byte that
        // is no UTF-8 at all is passed over, or the viewer would stand still on it.
        let (whole, skipped) = match std::str::from_utf8(&bytes) {
            Ok(_) => (bytes.len(), 0),
            Err(err) => (err.valid_up_to(), err.error_len().unwrap_or(0)),
        };
        self.offset += (whole + skipped) as u64;
        self.take(&String::from_utf8_lossy(&bytes[..whole]));
        self.offset < len && whole + skipped > 0
    }
}

/// Follows the log at `path` on the terminal until the owner quits (q, Esc, Ctrl+C).
pub async fn watch(home: PathBuf, path: PathBuf) -> std::process::ExitCode {
    let prefs = prefs::Prefs::load(&home);
    if let Some(name) = &prefs.theme {
        theme::set(name);
    }
    crate::texts::apply(prefs.language.as_deref(), None);
    let mut followed = Followed::default();
    followed.transcript.thinking = prefs.thinking.unwrap_or_default();
    let enhanced = match screen::enter() {
        Ok(enhanced) => enhanced,
        Err(err) => {
            screen::leave(false);
            eprintln!("glut: {}", tr!(terminal_unusable, reason = err));
            return std::process::ExitCode::from(1);
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let mut keys = screen::terminal_events(Arc::clone(&stop), Arc::default());
    let ended = follow_loop(&mut followed, &path, &mut keys).await;
    stop.store(true, Ordering::Relaxed);
    screen::leave(enhanced);
    match ended {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("glut: {err}");
            std::process::ExitCode::from(1)
        }
    }
}

async fn follow_loop(
    followed: &mut Followed,
    path: &Path,
    keys: &mut tokio::sync::mpsc::UnboundedReceiver<event::Event>,
) -> io::Result<()> {
    execute!(io::stdout(), Clear(ClearType::All))?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut poll = tokio::time::interval(POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut view = View::default();
    // More of the log is there than the last look read.
    let mut behind = false;
    let name = path.display().to_string();
    loop {
        execute!(io::stdout(), BeginSynchronizedUpdate)?;
        let drawn = terminal.draw(|frame| draw(frame, followed, &name, &mut view));
        execute!(io::stdout(), EndSynchronizedUpdate)?;
        drawn?;
        tokio::select! {
            biased;
            key = keys.recv() => match key {
                None => return Ok(()),
                Some(event::Event::Key(key)) if key.kind != KeyEventKind::Release => {
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('c') if ctrl => return Ok(()),
                        KeyCode::Up | KeyCode::Char('k') => view.scroll += 1,
                        KeyCode::Down | KeyCode::Char('j') => view.scroll = view.scroll.saturating_sub(1),
                        KeyCode::PageUp => view.scroll += PAGE,
                        KeyCode::PageDown => view.scroll = view.scroll.saturating_sub(PAGE),
                        KeyCode::Home | KeyCode::Char('g') => view.scroll = usize::MAX,
                        KeyCode::End | KeyCode::Char('G') => view.scroll = 0,
                        _ => {}
                    }
                }
                Some(event::Event::Mouse(mouse)) => match mouse.kind {
                    MouseEventKind::ScrollUp => view.scroll += 3,
                    MouseEventKind::ScrollDown => view.scroll = view.scroll.saturating_sub(3),
                    _ => {}
                },
                Some(_) => {}
            },
            // One part per pass, the keys first: a long log is drawn as it comes in, and q still quits.
            _ = poll.tick(), if !behind => behind = followed.look(path),
            () = std::future::ready(()), if behind => behind = followed.look(path),
        }
    }
}

/// Where the reader is in the stream.
#[derive(Default)]
struct View {
    /// Lines up from the end; 0 follows what comes.
    scroll: usize,
    /// The stream's lines at the last frame.
    drawn: usize,
}

/// The header, the stream from the view's place, and the keys.
fn draw(frame: &mut ratatui::Frame, followed: &mut Followed, name: &str, view: &mut View) {
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    let state = if followed.full {
        tr!(watch_log_full)
    } else if followed.working {
        tr!(watch_working)
    } else {
        tr!(watch_idle)
    };
    let shown = if followed.title.is_empty() {
        name
    } else {
        &followed.title
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("Glut · {} ", tr!(watching)), theme::quiet()),
            Span::styled(shown.to_owned(), theme::strong()),
            Span::styled(format!(" — {} · ", tr!(read_only)), theme::quiet()),
            Span::styled(
                state,
                if followed.working {
                    theme::glow()
                } else {
                    theme::quiet()
                },
            ),
        ])),
        head,
    );
    let thinking = followed.transcript.thinking;
    // Counted on every block (each keeps its lines made), copied only where they are in view.
    let total: usize = followed
        .transcript
        .blocks
        .iter_mut()
        .map(|block| block.lines(body.width, thinking).len())
        .sum();
    // What came while the reader is further up goes below: the lines they read stay where they are.
    if view.scroll > 0 && total > view.drawn {
        view.scroll += total - view.drawn;
    }
    view.drawn = total;
    let height = usize::from(body.height);
    let most = total.saturating_sub(height);
    view.scroll = view.scroll.min(most);
    let scroll = view.scroll;
    let start = most - scroll;
    let end = (start + height).min(total);
    let visible = if total == 0 {
        vec![Line::styled(tr!(watch_waiting), theme::quiet())]
    } else {
        visible_lines(&mut followed.transcript, body.width, start, end)
    };
    frame.render_widget(Paragraph::new(visible), body);
    let place = if scroll == 0 {
        tr!(watch_following).to_owned()
    } else {
        tr!(watch_lines_up, n = scroll)
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            tr!(
                watch_footer,
                input = followed.input_tokens,
                output = followed.output_tokens,
                place = place
            ),
            theme::quiet(),
        )),
        foot,
    );
}

/// The stream's lines `start..end` at `width`, copying no others.
fn visible_lines(
    transcript: &mut Transcript,
    width: u16,
    start: usize,
    end: usize,
) -> Vec<Line<'static>> {
    let thinking = transcript.thinking;
    let mut at = 0;
    let mut shown = Vec::with_capacity(end - start);
    for block in &mut transcript.blocks {
        let lines = block.lines(width, thinking);
        let next = at + lines.len();
        if next > start && at < end {
            shown.extend_from_slice(&lines[start.saturating_sub(at)..(end - at).min(lines.len())]);
        }
        if next >= end {
            break;
        }
        at = next;
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("glut-tui-follow-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_log_is_followed_line_by_line_and_a_torn_line_waits_for_its_end() {
        let mut followed = Followed::default();
        followed.take("{\"event\":\"owner\",\"text\":\"Fix the parser\",\"at\":\"x\"}\n{\"event\":\"assistant_te");
        assert_eq!(followed.title, "Fix the parser");
        assert!(followed.working);
        assert_eq!(followed.transcript.blocks.len(), 1);
        followed.take("xt\",\"turn\":1,\"text\":\"Fixed.\"}\n");
        assert_eq!(followed.transcript.blocks.len(), 2);
        followed.take(
            "{\"event\":\"usage\",\"turn\":1,\"input_tokens\":10,\"cached_input_tokens\":0,\"output_tokens\":3}\n",
        );
        assert_eq!((followed.input_tokens, followed.output_tokens), (10, 3));
        followed.take("not a line of the log\n");
        assert_eq!(followed.transcript.blocks.len(), 2);
    }

    #[test]
    fn a_file_is_read_as_it_grows_and_from_its_start_again_when_cut_back() {
        let dir = temp("grow");
        let path = dir.join("log.jsonl");
        let mut followed = Followed::default();
        assert!(!followed.look(&path), "no file yet: nothing");
        std::fs::write(&path, "{\"event\":\"owner\",\"text\":\"One\"}\n").unwrap();
        followed.look(&path);
        assert!(!followed.look(&path), "nothing new");
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{{\"event\":\"owner\",\"text\":\"Two\"}}").unwrap();
        followed.look(&path);
        assert_eq!(followed.transcript.blocks.len(), 2);
        std::fs::write(&path, "{\"event\":\"owner\",\"text\":\"Anew\"}\n").unwrap();
        followed.look(&path);
        assert_eq!(followed.title, "Anew");
        assert_eq!(followed.transcript.blocks.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_named_file_an_id_or_the_folder_s_newest_session_is_watched() {
        let home = temp("resolve");
        let cwd = home.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        assert!(resolve(&home, &cwd, None).is_err(), "no session yet");
        // A session whose first turn still runs: not kept yet, its log is there.
        let session = store::Session::new(&cwd, "p", "m");
        let running = log::EventLog::new(log::Target::Sessions(home.clone()), cwd.clone());
        running.follow(&session.id);
        running.owner("Fix the parser");
        assert_eq!(
            resolve(&home, &cwd, None).unwrap(),
            log::session_log(&home, &session.id)
        );
        assert_eq!(
            resolve(&home, &cwd, Some(&session.id)).unwrap(),
            log::session_log(&home, &session.id)
        );
        assert!(resolve(&home, &cwd, Some("no-such")).is_err());
        assert!(resolve(&home, &cwd, Some("../../etc/passwd")).is_err());
        let file = home.join("glut-events.jsonl");
        std::fs::write(&file, "").unwrap();
        let named = file.display().to_string();
        assert_eq!(resolve(&home, &cwd, Some(&named)).unwrap(), file);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_long_log_comes_in_parts_a_line_across_two_whole_and_a_bad_byte_is_passed_over() {
        let dir = temp("chunks");
        let path = dir.join("log.jsonl");
        let line = format!(
            "{{\"event\":\"owner\",\"text\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let count = usize::try_from(CHUNK).unwrap() / line.len() + 2;
        let mut text = line.repeat(count).into_bytes();
        text.extend_from_slice(b"\xff\n");
        text.extend_from_slice(b"{\"event\":\"owner\",\"text\":\"last\"}\n");
        std::fs::write(&path, &text).unwrap();
        let mut followed = Followed::default();
        assert!(followed.look(&path), "more than one part: more is left");
        while followed.look(&path) {}
        let owners = followed
            .transcript
            .blocks
            .iter()
            .filter(|block| matches!(block.kind, Kind::Owner(_)))
            .count();
        assert_eq!(owners, count + 1, "no line lost across the parts");
        assert_eq!(followed.offset, text.len() as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_lines_in_view_are_copied() {
        let mut followed = Followed::default();
        for n in 0..5 {
            followed.take(&format!("{{\"event\":\"owner\",\"text\":\"m{n}\"}}\n"));
        }
        let all: Vec<_> = followed
            .transcript
            .blocks
            .iter_mut()
            .flat_map(|block| block.lines(40, Default::default()).to_vec())
            .collect();
        let shown = visible_lines(&mut followed.transcript, 40, 2, all.len() - 1);
        assert_eq!(shown, all[2..all.len() - 1]);
    }
}
