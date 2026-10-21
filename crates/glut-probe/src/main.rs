//! `glut-probe`: what a terminal offers a full-screen TUI (`docs/plans/glut-tui.md`, TUI0).
//!
//! Two parts. First the terminal is asked what it is and what it can do — queries it answers by itself, each followed
//! by a primary device attributes request (`CSI c`) that every terminal answers: an answer that arrives before it is the
//! query's, none means the terminal does not know the query. Then a short guided test of what only a person can see or
//! do: colours, styles, wide characters, a link, the alternate screen, tearing, keys, the mouse, a paste, the
//! clipboard, a resize. Everything ends in a Markdown report in the current folder.
//!
//! Nothing here is kept for the TUI itself; the report is what TUI0 delivers.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor};
use crossterm::terminal::{
    self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{cursor, execute, queue};

/// How long the terminal has to answer one query; a terminal over SSH may need a moment.
const ANSWER_WAIT: Duration = Duration::from_millis(1500);
/// How long a guided step waits for the person before it is counted as skipped.
const STEP_WAIT: Duration = Duration::from_secs(45);
/// The primary device attributes request: every terminal answers it, so it closes every query.
const DA1: &str = "\x1b[c";
/// What the clipboard test puts on the clipboard and expects back as a paste.
const CLIPBOARD_TEXT: &str = "glut-probe-osc52-7f3a";

/// One finding of the report.
struct Finding {
    what: String,
    result: String,
}

#[derive(Default)]
struct Report {
    findings: Vec<Finding>,
}

impl Report {
    fn add(&mut self, what: &'static str, result: impl Into<String>) {
        self.add_owned(what.to_owned(), result.into());
    }

    fn add_owned(&mut self, what: String, result: String) {
        self.findings.push(Finding { what, result });
    }
}

/// The parts a run measures: all of them, or those `--only a,b` names (to measure one part again after a fix).
struct Parts(Option<Vec<String>>);

impl Parts {
    /// The parts named on the command line; an unknown name ends the probe before it touches the terminal.
    fn from_args() -> io::Result<Self> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.as_slice() {
            [] => Ok(Self(None)),
            [flag, names] if flag == "--only" => {
                let names: Vec<String> = names
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .collect();
                if let Some(unknown) = names.iter().find(|name| !PARTS.contains(&name.as_str())) {
                    return Err(io::Error::other(format!(
                        "unknown part {unknown}; the parts are {}",
                        PARTS.join(", ")
                    )));
                }
                Ok(Self(Some(names)))
            }
            _ => Err(io::Error::other(format!(
                "usage: glut-probe [--only part,part]; the parts are {}",
                PARTS.join(", ")
            ))),
        }
    }

    fn wants(&self, part: &str) -> bool {
        self.0
            .as_ref()
            .is_none_or(|names| names.iter().any(|name| name == part))
    }

    /// The report's name suffix: nothing for a full run, `-only-keys-tearing` for a partial one.
    fn suffix(&self) -> String {
        self.0
            .as_ref()
            .map(|names| format!("-only-{}", names.join("-")))
            .unwrap_or_default()
    }
}

/// Every part `--only` may name.
const PARTS: [&str; 11] = [
    "queries",
    "colours",
    "styles",
    "wide",
    "link",
    "tearing",
    "keys",
    "mouse",
    "paste",
    "resize",
    "altscreen",
];

fn main() -> io::Result<()> {
    let parts = Parts::from_args()?;
    let mut report = Report::default();
    environment(&mut report);
    terminal::enable_raw_mode()?;
    let outcome = (|| -> io::Result<()> {
        if parts.wants("queries") {
            queries(&mut report)?;
        }
        guided(&mut report, &parts)
    })();
    // Whatever happened, the terminal is given back as it was.
    let mut out = io::stdout();
    let _ = execute!(
        out,
        PopKeyboardEnhancementFlags,
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
        ResetColor,
        cursor::Show
    );
    terminal::disable_raw_mode()?;
    outcome?;
    let path = write_report(&report, &parts.suffix())?;
    println!("\nReport written to {}", path.display());
    Ok(())
}

/// What the environment says about the terminal.
fn environment(report: &mut Report) {
    for (what, name) in [
        ("TERM", "TERM"),
        ("TERM_PROGRAM", "TERM_PROGRAM"),
        ("TERM_PROGRAM_VERSION", "TERM_PROGRAM_VERSION"),
        ("COLORTERM", "COLORTERM"),
        ("inside tmux (TMUX)", "TMUX"),
        ("over SSH (SSH_TTY)", "SSH_TTY"),
    ] {
        report.add(what, std::env::var(name).unwrap_or_else(|_| "—".into()));
    }
    let size = terminal::size()
        .map(|(cols, rows)| format!("{cols} × {rows}"))
        .unwrap_or_else(|err| format!("unknown ({err})"));
    report.add("size (columns × rows)", size);
}

/// The queries a terminal answers by itself.
fn queries(report: &mut Report) -> io::Result<()> {
    let named = [
        ("version (XTVERSION)", "\x1b[>q"),
        ("secondary device attributes (DA2)", "\x1b[>c"),
        ("synchronized output ?2026 (DECRQM)", "\x1b[?2026$p"),
        ("alternate screen ?1049 (DECRQM)", "\x1b[?1049$p"),
        ("bracketed paste ?2004 (DECRQM)", "\x1b[?2004$p"),
        ("SGR mouse ?1006 (DECRQM)", "\x1b[?1006$p"),
        ("focus events ?1004 (DECRQM)", "\x1b[?1004$p"),
        ("kitty keyboard protocol (CSI ? u)", "\x1b[?u"),
        ("background colour (OSC 11)", "\x1b]11;?\x07"),
        ("foreground colour (OSC 10)", "\x1b]10;?\x07"),
        ("cursor position (DSR 6)", "\x1b[6n"),
        ("cell size in pixels (CSI 16 t)", "\x1b[16t"),
        (
            "kitty graphics (APC G query)",
            "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\",
        ),
    ];
    let da1 = ask("")?;
    report.add("primary device attributes (DA1)", describe(&da1));
    for (what, query) in named {
        let answer = ask(query)?;
        report.add(what, interpret(query, &answer));
    }
    Ok(())
}

/// Sends `query` and the closing `DA1`, and returns what came back before the DA1 answer — or, for an empty query, the
/// DA1 answer itself. `None` when nothing came in time.
fn ask(query: &str) -> io::Result<Option<String>> {
    let mut out = io::stdout();
    write!(out, "{query}{DA1}")?;
    out.flush()?;
    let stdin = io::stdin();
    let fd = stdin.as_fd();
    let until = Instant::now() + ANSWER_WAIT;
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < until {
        let left = until.saturating_duration_since(Instant::now());
        let timeout = rustix::event::Timespec {
            tv_sec: left.as_secs() as i64,
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
            break;
        }
        let read = rustix::io::read(fd, &mut buf)?;
        if read == 0 {
            break;
        }
        got.extend_from_slice(&buf[..read]);
        let text = String::from_utf8_lossy(&got).into_owned();
        if let Some(at) = da1_answer_at(&text) {
            return Ok(Some(if query.is_empty() {
                text[at..].to_owned()
            } else {
                text[..at].to_owned()
            }));
        }
    }
    Ok((!got.is_empty()).then(|| String::from_utf8_lossy(&got).into_owned()))
}

/// Where the DA1 answer (`CSI ? … c`) starts in `text`, once it is complete.
fn da1_answer_at(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find("\x1b[?") {
        let start = from + found;
        let rest = &text[start + 3..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
        if rest[end..].starts_with('c') {
            return Some(start);
        }
        from = start + 3;
    }
    None
}

/// What an answer means for its query.
fn interpret(query: &str, answer: &Option<String>) -> String {
    let Some(answer) = answer.as_deref().filter(|a| !a.is_empty()) else {
        return "no answer (not supported, or the answer was swallowed on the way)".into();
    };
    if query.contains("$p") {
        // DECRPM: CSI ? mode ; value $ y — 1 set, 2 reset (both: known), 3/4 permanently set/reset, 0 unknown.
        let value = answer
            .rsplit(';')
            .next()
            .and_then(|tail| tail.trim_end_matches("$y").parse::<u8>().ok());
        return match value {
            Some(1) => "supported, on".into(),
            Some(2) => "supported, off".into(),
            Some(3) => "permanently on".into(),
            Some(4) => "permanently off".into(),
            Some(0) => "not recognised".into(),
            _ => format!("unexpected answer {}", describe(&Some(answer.to_owned()))),
        };
    }
    describe(&Some(answer.to_owned()))
}

/// An answer as readable text: escape characters spelled out.
fn describe(answer: &Option<String>) -> String {
    let Some(answer) = answer else {
        return "no answer".into();
    };
    answer
        .chars()
        .map(|c| match c {
            '\x1b' => "ESC".to_owned(),
            '\x07' => "BEL".to_owned(),
            c if c.is_control() => format!("\\x{:02x}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

/// The guided part: what only a person can see or do.
fn guided(report: &mut Report, parts: &Parts) -> io::Result<()> {
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    if parts.wants("colours") {
        report.add("truecolor gradient smooth", colours(&mut out)?);
    }
    if parts.wants("styles") {
        styles(&mut out, report)?;
    }
    if parts.wants("wide") {
        let widths = show(&mut out, "Wide characters", |out| {
            queue!(
                out,
                Print("|⬡ hex|日本|🙂 x|é|\r\n"),
                Print("|----|----|----|-|\r\n"),
                Print("┌──┬──┐\r\n│ab│cd│\r\n└──┴──┘")
            )?;
            Ok("Do the bars | line up in both rows, and is the box closed?")
        })?;
        report.add("wide characters and box drawing aligned", widths);
    }
    if parts.wants("link") {
        let link = show(&mut out, "Link (OSC 8)", |out| {
            queue!(
                out,
                Print(
                    "\x1b]8;;https://example.com\x1b\\this text is a link to example.com\x1b]8;;\x1b\\"
                )
            )?;
            Ok("Can you click (or Cmd-click) the text to open the link?")
        })?;
        report.add("hyperlinks (OSC 8) clickable", link);
    }
    if parts.wants("tearing") {
        report.add(
            "half frames without synchronized output",
            tearing(&mut out, false)?,
        );
        report.add(
            "half frames with synchronized output",
            tearing(&mut out, true)?,
        );
    }
    if parts.wants("keys") {
        keys(&mut out, report)?;
    }
    if parts.wants("mouse") {
        mouse(&mut out, report)?;
    }
    if parts.wants("paste") {
        paste_and_clipboard(&mut out, report)?;
    }
    if parts.wants("resize") {
        let resized = step(
            &mut out,
            "Resize",
            "Change the window's size now (or press s to skip).",
            |event| match event {
                Event::Resize(cols, rows) => Some(format!("reported: {cols} × {rows}")),
                _ => None,
            },
        )?;
        report.add("resize reported", resized);
    }
    execute!(out, LeaveAlternateScreen)?;
    if parts.wants("altscreen") {
        let back = question(
            "Back on the normal screen: is what was there before the probe visible again (y/n)?",
        )?;
        report.add("alternate screen restores the screen", back);
    }
    Ok(())
}

/// A truecolor gradient and the 256-colour cube.
fn colours(out: &mut io::Stdout) -> io::Result<String> {
    show(out, "Colours", |out| {
        queue!(out, Print("A smooth gradient (truecolor):\r\n"))?;
        for i in 0..72u8 {
            let r = 255 - i * 3;
            let b = i * 3 + 40;
            queue!(
                out,
                SetBackgroundColor(Color::Rgb { r, g: 90, b }),
                Print(" ")
            )?;
        }
        queue!(out, ResetColor, Print("\r\n\r\nThe 256-colour cube:\r\n"))?;
        for i in 16..88u8 {
            queue!(out, SetBackgroundColor(Color::AnsiValue(i)), Print(" "))?;
        }
        queue!(out, ResetColor)?;
        Ok("Is the first row a smooth gradient without visible steps?")
    })
}

/// The styles a TUI uses, numbered: the person names the ones that do not look as they say, so each gets a finding.
fn styles(out: &mut io::Stdout, report: &mut Report) -> io::Result<()> {
    queue!(
        out,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        SetAttribute(Attribute::Bold),
        Print("Styles"),
        SetAttribute(Attribute::Reset),
        Print("\r\n\r\n")
    )?;
    for (number, (attribute, name)) in (1..).zip(STYLES) {
        queue!(
            out,
            Print(format!("{number}  ")),
            SetAttribute(attribute),
            Print(name),
            SetAttribute(Attribute::Reset),
            Print("\r\n")
        )?;
    }
    queue!(
        out,
        Print(format!("{}  ", STYLES.len() + 1)),
        Print("\x1b[4:3m\x1b[58:2::255:120:40mcurly orange underline\x1b[59m\x1b[24m\r\n\r\n"),
        Print(
            "Type the numbers of the lines that do NOT look as they say, then Enter (only Enter: all fine).  "
        )
    )?;
    out.flush()?;
    let wrong = digits_until_enter(out)?;
    for (number, (_, name)) in (1..).zip(STYLES) {
        report.add_owned(
            format!("style {name}"),
            style_verdict(wrong.as_deref(), number),
        );
    }
    report.add_owned(
        "style curly orange underline (SGR 4:3 + 58)".into(),
        style_verdict(wrong.as_deref(), STYLES.len() + 1),
    );
    Ok(())
}

/// The styles the probe shows, each with its name.
const STYLES: [(Attribute, &str); 7] = [
    (Attribute::Bold, "bold"),
    (Attribute::Dim, "dim"),
    (Attribute::Italic, "italic"),
    (Attribute::Underlined, "underlined"),
    (Attribute::Undercurled, "curly underline"),
    (Attribute::CrossedOut, "crossed out"),
    (Attribute::Reverse, "reverse"),
];

/// A style's finding from the numbers the person typed; `None` when no answer came.
fn style_verdict(wrong: Option<&[usize]>, number: usize) -> String {
    match wrong {
        None => "no answer in time".into(),
        Some(wrong) if wrong.contains(&number) => "looks wrong".into(),
        Some(_) => "yes".into(),
    }
}

/// The digits typed (echoed) until Enter; `None` when nothing ends the line in time.
fn digits_until_enter(out: &mut io::Stdout) -> io::Result<Option<Vec<usize>>> {
    let mut typed = Vec::new();
    let until = Instant::now() + STEP_WAIT;
    while Instant::now() < until {
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        else {
            continue;
        };
        match code {
            KeyCode::Enter => return Ok(Some(typed)),
            KeyCode::Char(c) if c.is_ascii_digit() => {
                typed.push(usize::from(c as u8 - b'0'));
                execute!(out, Print(c))?;
            }
            _ => {}
        }
    }
    Ok(None)
}

/// Draws a test with `draw` under a title and asks its question; the answer is y, n or skipped.
fn show(
    out: &mut io::Stdout,
    title: &str,
    draw: impl FnOnce(&mut io::Stdout) -> io::Result<&'static str>,
) -> io::Result<String> {
    queue!(
        out,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        SetAttribute(Attribute::Bold),
        Print(title)
    )?;
    queue!(out, SetAttribute(Attribute::Reset), Print("\r\n\r\n"))?;
    let asked = draw(out)?;
    queue!(out, ResetColor, Print("\r\n\r\n"))?;
    out.flush()?;
    question(asked)
}

/// Asks `text` and waits for y or n.
fn question(text: &str) -> io::Result<String> {
    let mut out = io::stdout();
    execute!(out, Print(format!("{text}  [y/n, s skips] ")))?;
    let until = Instant::now() + STEP_WAIT;
    while Instant::now() < until {
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        if let Event::Key(KeyEvent {
            code: KeyCode::Char(c),
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        {
            match c {
                'y' | 'j' => return Ok("yes".into()),
                'n' => return Ok("no".into()),
                's' => return Ok("skipped".into()),
                _ => {}
            }
        }
    }
    Ok("no answer in time".into())
}

/// A step that waits for an event `seen` recognises; s skips.
fn step(
    out: &mut io::Stdout,
    title: &str,
    text: &str,
    mut seen: impl FnMut(&Event) -> Option<String>,
) -> io::Result<String> {
    execute!(
        out,
        Clear(ClearType::All),
        cursor::MoveTo(0, 0),
        SetAttribute(Attribute::Bold),
        Print(title),
        SetAttribute(Attribute::Reset),
        Print("\r\n\r\n"),
        Print(text),
        Print("\r\n")
    )?;
    let until = Instant::now() + STEP_WAIT;
    while Instant::now() < until {
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let event = event::read()?;
        if let Event::Key(KeyEvent {
            code: KeyCode::Char('s'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            return Ok("skipped".into());
        }
        if let Some(found) = seen(&event) {
            return Ok(found);
        }
    }
    Ok("nothing in time".into())
}

/// Redraws the screen slowly in two halves, with or without synchronized output, and asks whether half frames showed.
///
/// Each frame writes its top half, waits, then its bottom half: without synchronized output a terminal shows the half
/// frame in between (top new, bottom old), with it a terminal that honours `?2026` holds the screen until the frame is
/// whole. A fast full-screen flip would only show flicker, which nobody can tell from tearing by eye.
fn tearing(out: &mut io::Stdout, synchronized: bool) -> io::Result<String> {
    let (cols, rows) = terminal::size()?;
    let rows = rows.saturating_sub(2);
    for frame in 0..HALF_FRAME_COUNT {
        if synchronized {
            queue!(out, BeginSynchronizedUpdate)?;
        }
        let fill = if frame % 2 == 0 { '█' } else { '░' };
        let line: String = std::iter::repeat_n(fill, usize::from(cols)).collect();
        for row in 0..rows / 2 {
            queue!(out, cursor::MoveTo(0, row), Print(&line))?;
        }
        out.flush()?;
        std::thread::sleep(HALF_FRAME_PAUSE);
        for row in rows / 2..rows {
            queue!(out, cursor::MoveTo(0, row), Print(&line))?;
        }
        if synchronized {
            queue!(out, EndSynchronizedUpdate)?;
        }
        out.flush()?;
        std::thread::sleep(HALF_FRAME_PAUSE);
    }
    queue!(out, Clear(ClearType::All), cursor::MoveTo(0, 0))?;
    let how = if synchronized { "with" } else { "without" };
    question(&format!(
        "That was {HALF_FRAME_COUNT} slow redraws {how} synchronized output. Was the screen ever split — top half one \
         pattern, bottom half the other (y) — or did it always change as a whole (n)?"
    ))
    .map(|answer| match answer.as_str() {
        "yes" => "half frames seen".to_owned(),
        "no" => "always whole".to_owned(),
        other => other.to_owned(),
    })
}

/// How many frames the tearing test draws.
const HALF_FRAME_COUNT: u16 = 16;
/// The pause between a frame's two halves, and after it: long enough to see the half frame.
const HALF_FRAME_PAUSE: Duration = Duration::from_millis(150);

/// Which keys arrive, and how, with the kitty keyboard protocol on when the terminal has it.
fn keys(out: &mut io::Stdout, report: &mut Report) -> io::Result<()> {
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            out,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )?;
    }
    report.add(
        "kitty keyboard enhancement pushed",
        if enhanced {
            "yes"
        } else {
            "no (not supported)"
        },
    );
    for (name, wanted) in [
        ("Shift+Tab", "the plan/build switch"),
        ("Shift+Enter", "a new line in the input"),
        ("Alt+Enter", "queue a message"),
        ("Ctrl+Enter", "send"),
        ("Esc", "stop"),
        ("Ctrl+K", "the palette"),
        ("F2", "hand the mouse back"),
        ("Alt+Up", "take a queued message back"),
    ] {
        // What the last combination left behind (its release, a modifier) must not count for this one.
        drain()?;
        let found = step(
            out,
            "Keys",
            &format!("Press {name} ({wanted}) — or s to skip."),
            |event| match event {
                Event::Key(key) => key_seen(key),
                _ => None,
            },
        )?;
        report.add(key_label(name), found);
    }
    if enhanced {
        execute!(out, PopKeyboardEnhancementFlags)?;
    }
    Ok(())
}

/// A key event as the report names it; `None` for what is no key of its own: a release, a repeat, or a modifier
/// pressed alone (the kitty protocol with all keys as escape codes reports Shift, Alt and Ctrl by themselves).
fn key_seen(key: &KeyEvent) -> Option<String> {
    if key.kind != KeyEventKind::Press || matches!(key.code, KeyCode::Modifier(_)) {
        return None;
    }
    Some(format!("{:?} with {:?}", key.code, key.modifiers))
}

/// Reads and drops every event already waiting.
fn drain() -> io::Result<()> {
    std::thread::sleep(Duration::from_millis(150));
    while event::poll(Duration::ZERO)? {
        event::read()?;
    }
    Ok(())
}

/// The report's label for a key.
fn key_label(name: &str) -> &'static str {
    match name {
        "Shift+Tab" => "key Shift+Tab arrives as",
        "Shift+Enter" => "key Shift+Enter arrives as",
        "Alt+Enter" => "key Alt+Enter arrives as",
        "Ctrl+Enter" => "key Ctrl+Enter arrives as",
        "Esc" => "key Esc arrives as",
        "Ctrl+K" => "key Ctrl+K arrives as",
        "F2" => "key F2 arrives as",
        _ => "key Alt+Up arrives as",
    }
}

/// A click, a drag and the wheel, with mouse capture on.
fn mouse(out: &mut io::Stdout, report: &mut Report) -> io::Result<()> {
    execute!(out, EnableMouseCapture, EnableFocusChange)?;
    let click = step(
        out,
        "Mouse",
        "Click anywhere in this window — or s to skip.",
        |event| match event {
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)) => Some(format!(
                "click at column {}, row {}",
                mouse.column, mouse.row
            )),
            _ => None,
        },
    )?;
    report.add("mouse click reported", click);
    let mut moves = 0;
    let drag = step(
        out,
        "Mouse",
        "Hold the button and drag across a few cells — or s to skip.",
        |event| match event {
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Drag(_)) => {
                moves += 1;
                (moves >= 3).then(|| format!("drag reported ({moves} moves)"))
            }
            _ => None,
        },
    )?;
    report.add("mouse drag reported", drag);
    let wheel = step(
        out,
        "Mouse",
        "Turn the scroll wheel (or scroll on the trackpad) — or s to skip.",
        |event| match event {
            Event::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                ) =>
            {
                Some(format!("{:?}", mouse.kind))
            }
            _ => None,
        },
    )?;
    report.add("mouse wheel reported", wheel);
    let focus = step(
        out,
        "Focus",
        "Switch to another window and back — or s to skip.",
        |event| match event {
            Event::FocusLost => Some("focus lost reported".into()),
            Event::FocusGained => Some("focus gained reported".into()),
            _ => None,
        },
    )?;
    report.add("focus events reported", focus);
    execute!(out, DisableMouseCapture, DisableFocusChange)?;
    Ok(())
}

/// A bracketed paste of several lines, then the clipboard written over OSC 52 and pasted back.
fn paste_and_clipboard(out: &mut io::Stdout, report: &mut Report) -> io::Result<()> {
    execute!(out, EnableBracketedPaste)?;
    let pasted = step(
        out,
        "Paste",
        "Copy two or more lines of text anywhere, then paste them here (Cmd+V) — or s to skip.",
        |event| match event {
            Event::Paste(text) => Some(format!(
                "arrived as one paste: {} lines, {} characters",
                text.lines().count(),
                text.chars().count()
            )),
            Event::Key(_) => {
                Some("arrived as key presses, not as a paste (no bracketed paste)".into())
            }
            _ => None,
        },
    )?;
    report.add("bracketed paste", pasted);
    let mut encoded = String::new();
    base64_into(CLIPBOARD_TEXT.as_bytes(), &mut encoded);
    execute!(out, Print(format!("\x1b]52;c;{encoded}\x07")))?;
    let clipboard = step(
        out,
        "Clipboard (OSC 52)",
        "The probe just put a word on the clipboard by itself. Paste it here (Cmd+V) — or s to skip.",
        |event| match event {
            Event::Paste(text) if text.trim() == CLIPBOARD_TEXT => Some("works".into()),
            Event::Paste(_) => {
                Some("the clipboard still held something else: OSC 52 not honoured".into())
            }
            _ => None,
        },
    )?;
    report.add("clipboard over OSC 52", clipboard);
    execute!(out, DisableBracketedPaste)?;
    Ok(())
}

/// Standard base64 of `bytes`, appended to `out` (OSC 52's encoding; no dependency for twenty lines).
fn base64_into(bytes: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
}

/// Writes the report into the current folder, named after the terminal, and returns its path.
fn write_report(report: &Report, suffix: &str) -> io::Result<std::path::PathBuf> {
    let terminal_name = std::env::var("TERM_PROGRAM")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "terminal".into());
    let slug: String = terminal_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut text = format!("# glut-probe — {terminal_name}\n\n| what | result |\n|---|---|\n");
    for finding in &report.findings {
        let result = finding.result.replace('|', "\\|");
        let _ = writeln!(text, "| {} | {} |", finding.what, result);
    }
    let path = std::env::current_dir()?.join(format!("glut-probe-{slug}{suffix}.md"));
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_da1_answer_is_found_only_when_complete_and_after_another_answer() {
        assert_eq!(da1_answer_at("\x1b[?62;22c"), Some(0));
        assert_eq!(da1_answer_at("\x1b[?2026;2$y\x1b[?62;4c"), Some(11));
        assert_eq!(da1_answer_at("\x1b[?62;4"), None, "not complete yet");
        assert_eq!(
            da1_answer_at("\x1b[?1u"),
            None,
            "a kitty keyboard answer is no DA1"
        );
    }

    #[test]
    fn a_mode_report_says_whether_the_mode_is_known() {
        let answer = |text: &str| Some(text.to_owned());
        assert_eq!(
            interpret("\x1b[?2026$p", &answer("\x1b[?2026;2$y")),
            "supported, off"
        );
        assert_eq!(
            interpret("\x1b[?2026$p", &answer("\x1b[?2026;0$y")),
            "not recognised"
        );
        assert!(interpret("\x1b[?2026$p", &None).starts_with("no answer"));
    }

    #[test]
    fn a_modifier_alone_a_release_or_a_repeat_is_no_key_of_its_own() {
        use crossterm::event::{KeyEventState, ModifierKeyCode};
        let key = |code, modifiers, kind| KeyEvent {
            code,
            modifiers,
            kind,
            state: KeyEventState::NONE,
        };
        let shift = KeyModifiers::SHIFT;
        assert_eq!(
            key_seen(&key(
                KeyCode::Modifier(ModifierKeyCode::LeftShift),
                shift,
                KeyEventKind::Press
            )),
            None
        );
        assert_eq!(
            key_seen(&key(KeyCode::Enter, shift, KeyEventKind::Release)),
            None
        );
        assert_eq!(
            key_seen(&key(KeyCode::Enter, shift, KeyEventKind::Repeat)),
            None
        );
        assert_eq!(
            key_seen(&key(KeyCode::Enter, shift, KeyEventKind::Press)).as_deref(),
            Some("Enter with KeyModifiers(SHIFT)")
        );
    }

    #[test]
    fn a_style_is_wrong_only_when_its_number_was_typed() {
        assert_eq!(style_verdict(Some(&[2, 7]), 2), "looks wrong");
        assert_eq!(style_verdict(Some(&[2, 7]), 3), "yes");
        assert_eq!(style_verdict(Some(&[]), 1), "yes");
        assert_eq!(style_verdict(None, 1), "no answer in time");
    }

    #[test]
    fn base64_matches_the_standard_alphabet_with_padding() {
        let encoded = |text: &str| {
            let mut out = String::new();
            base64_into(text.as_bytes(), &mut out);
            out
        };
        assert_eq!(encoded("glut"), "Z2x1dA==");
        assert_eq!(encoded("abc"), "YWJj");
        assert_eq!(encoded("ab"), "YWI=");
    }
}
