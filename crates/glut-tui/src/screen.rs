//! The screen (TUI1a, made a library's in TUI8a): takes the terminal, runs one session on it until the owner ends
//! it, and gives the terminal back — for the standalone `glut` and for a host that runs Glut for its own ends.

use std::io::{self, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use glut::Message;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

use crate::app::App;
use crate::host::{EngineChoice, Host, HostLink, Launch, State};
use crate::{prefs, scrollback, session, setup, store, transcript, vi, view};

/// How often the screen moves on its own while Glut works or Glimm is in view (its flicker, its breath).
const TICK: Duration = Duration::from_millis(120);
/// How long the terminal thread waits for a key before it looks whether it should stop.
const POLL: Duration = Duration::from_millis(100);
/// How many of the stream's last lines stay in the terminal when the session ends.
const EXIT_LINES: usize = 5_000;

/// How the session opens.
#[derive(Default)]
pub struct Open {
    /// Go on with the newest kept session of the folder (`glut -c`).
    pub continue_last: bool,
    /// Pick a kept session of the folder first (`glut -r`).
    pub pick: bool,
    /// A conversation the host kept, which the session goes on from.
    pub conversation: Vec<Message>,
    /// A first message, sent as soon as the session is up.
    pub prompt: Option<String>,
    /// A run of the host's that was cut off: it goes on first, from its journal ([`crate::Turn::Resumed`]), and the
    /// first message follows unless the run had it already.
    pub lost_turn: Option<String>,
}

/// The owner's own session, as the standalone `glut` runs it — also for a host that hands the owner Glut as it is, with
/// no part of its own (Axiomata's master pane, TUI8c): the first start asks for a provider and a missing model is
/// chosen before the screen comes up, then [`run`] with the [`Standalone`](crate::Standalone) host.
pub async fn standalone(launch: Launch, open: Open) -> std::process::ExitCode {
    if let Ok(home) = glut::permissions::settings::home_dir() {
        if let Err(reason) = setup::first_provider(&home).await {
            eprintln!("glut: {reason}");
            return std::process::ExitCode::from(1);
        }
        if let EngineChoice::Providers { provider, model } = &launch.engine
            && let Err(reason) =
                setup::missing_model(&home, provider.as_deref(), model.as_deref()).await
        {
            eprintln!("glut: {reason}");
            return std::process::ExitCode::from(1);
        }
    }
    run(launch, Arc::new(crate::Standalone), open, |_| {}).await
}

/// Runs one session on the terminal: built as `launch` says, with `host` taking part; `linked` gets the host's way
/// to the screen before the session starts. Says on stderr why it could not start, and ends with the session's
/// figures on stdout.
pub async fn run(
    launch: Launch,
    host: Arc<dyn Host>,
    open: Open,
    linked: impl FnOnce(HostLink),
) -> std::process::ExitCode {
    let (reports, mut from_session) = mpsc::unbounded_channel();
    eprintln!("Glut is starting…");
    let (parts, engine) = match session::build(launch, Arc::clone(&host), reports.clone()).await {
        Ok((parts, engine)) => (Arc::new(parts), engine),
        Err(reason) => {
            eprintln!("glut: {reason}");
            host.state(State::Ended);
            return std::process::ExitCode::from(1);
        }
    };
    linked(HostLink(reports.clone()));
    let mut app = App::new(parts, engine, reports);
    if open.continue_last {
        match store::Session::list(&app.parts.home, &app.parts.config.cwd)
            .into_iter()
            .next()
        {
            Some(session) => app.open(session),
            None => app.transcript.note(
                "no earlier session in this folder — a new one starts",
                transcript::Tone::Quiet,
            ),
        }
    }
    if !open.conversation.is_empty() {
        app.go_on_from(open.conversation);
    }
    if open.pick {
        app.picker = Some(app.session_picker());
    }
    match (open.lost_turn, open.prompt) {
        (Some(run), prompt) => app.resume(run, prompt),
        (None, Some(prompt)) => app.send(prompt),
        (None, None) => {}
    }
    let outcome = screen(&mut app, &mut from_session).await;
    host.state(State::Ended);
    outcome
}

/// The session on the terminal, from taking it to giving it back.
async fn screen(
    app: &mut App,
    from_session: &mut mpsc::UnboundedReceiver<session::Report>,
) -> std::process::ExitCode {
    let enhanced = match enter() {
        Ok(enhanced) => enhanced,
        Err(err) => {
            leave(false);
            eprintln!("glut: cannot use this terminal: {err}");
            return std::process::ExitCode::from(1);
        }
    };
    // A panic of the screen's own thread must not leave the owner's terminal in raw mode on the alternate screen —
    // whichever thread a host runs the screen on. A turn's panic is caught where the turn ends (`session::turn`) and
    // the screen goes on: it keeps the terminal. The hook before ours comes back when the screen ends.
    let screen_thread = std::thread::current().id();
    let previous: Arc<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync> =
        Arc::from(std::panic::take_hook());
    let chained = Arc::clone(&previous);
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == screen_thread {
            leave(enhanced);
        }
        chained(info);
    }));

    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(Pause::default());
    let mut keys = terminal_events(Arc::clone(&stop), Arc::clone(&pause));
    let outcome = run_loop(app, &mut keys, from_session, enhanced, &pause).await;
    stop.store(true, Ordering::Relaxed);
    leave(enhanced);
    let _ = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| previous(info)));
    // The session stays where the terminal keeps it, above the shell's next prompt — settled first (a turn stopped
    // by the quit shows as stopped), its last lines only: a terminal keeps a few thousand anyway. Printed after an
    // error too: the alternate screen it was on is gone.
    if !app.welcoming() {
        app.transcript.settle();
        let width = terminal::size().map_or(80, |(width, _)| width);
        let lines = app.stream_at(width);
        let tail = &lines[lines.len().saturating_sub(EXIT_LINES)..];
        let _ = scrollback::print(&mut io::stdout(), tail);
    }
    match outcome {
        Ok(()) => {
            // Written, not printed: a terminal that is gone (SIGHUP) must not turn the end into a panic.
            let _ = writeln!(
                io::stdout(),
                "Glut: session ended — {} tokens in, {} out{}",
                app.usage.input_tokens,
                app.usage.output_tokens,
                app.cost
                    .map(|cost| format!(", ${cost:.4}"))
                    .unwrap_or_default()
            );
            std::process::ExitCode::SUCCESS
        }
        Err(err) => {
            let _ = writeln!(io::stderr(), "glut: {err}");
            std::process::ExitCode::from(1)
        }
    }
}

/// The main loop: draws, then waits for a key, a step of the run or the next tick.
async fn run_loop(
    app: &mut App,
    keys: &mut mpsc::UnboundedReceiver<event::Event>,
    from_session: &mut mpsc::UnboundedReceiver<session::Report>,
    enhanced: bool,
    pause: &Pause,
) -> io::Result<()> {
    // The first frame starts from a blank screen, whatever the terminal showed before (a terminal without an
    // alternate screen keeps the old text, and ratatui would leave every cell it thinks blank as it was). Cleared
    // directly: ratatui's own clear asks the terminal for the cursor, which a terminal without answers (Axiomata's,
    // TUI0) never gives.
    execute!(io::stdout(), Clear(ClearType::All))?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut tick = tokio::time::interval(TICK);
    // Ticks missed while Glut waited are not made up in a burst when it starts working again.
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    // Raw mode sends no SIGINT; at the scrollback's prompt (cooked mode) Ctrl+C would end Glut mid-turn: it goes
    // back instead.
    let mut interrupt = signal(SignalKind::interrupt())?;
    // The cursor's shape follows the vi mode: a block in Normal mode, the owner's own shape otherwise.
    let mut block_cursor = false;
    loop {
        let wanted = app
            .vi
            .as_ref()
            .is_some_and(|vi| vi.mode == vi::Mode::Normal);
        if wanted != block_cursor {
            let shape = if wanted {
                cursor::SetCursorStyle::SteadyBlock
            } else {
                cursor::SetCursorStyle::DefaultUserShape
            };
            execute!(io::stdout(), shape)?;
            block_cursor = wanted;
        }
        app.drop_answered_ask();
        // Every frame inside synchronized output (DEC 2026): the terminal shows it whole or not at all — and the end
        // is sent even when the frame failed, or the terminal would hold every later one back.
        execute!(io::stdout(), BeginSynchronizedUpdate)?;
        let drawn = terminal.draw(|frame| view::draw(frame, app));
        execute!(io::stdout(), EndSynchronizedUpdate)?;
        drawn?;
        if app.quit {
            // A turn the host can go on with later is left to its journal when the terminal is gone.
            if !(app.abandoned && app.parts.keep_lost_turns) {
                app.stop_turn();
            }
            return Ok(());
        }
        // F2: the mouse to the terminal (its own selection) and back.
        if let Some(held) = app.mouse_request.take() {
            if held {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
            app.mouse_held = held;
            app.mouse_said(held);
        }
        // F3 or /scrollback: the stream where the terminal keeps it, until the owner comes back with Enter.
        if std::mem::take(&mut app.scrollback_request) {
            pause.ask.store(true, Ordering::SeqCst);
            while !pause.idle.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            leave(enhanced);
            let width = terminal::size().map_or(80, |(width, _)| width);
            let lines = app.stream_at(width);
            let shown = scrollback::print(&mut io::stdout(), &lines).and_then(|()| {
                print!(
                    "\n— Glut: the conversation is in your terminal's scrollback. Enter or Ctrl+C goes back. "
                );
                io::stdout().flush()
            });
            if shown.is_ok() {
                // The wait for Enter runs beside the signals: a closed window still ends the session cleanly.
                let line =
                    tokio::task::spawn_blocking(|| io::stdin().read_line(&mut String::new()));
                tokio::select! {
                    _ = line => {}
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => app.abandon(),
                    _ = hangup.recv() => app.abandon(),
                }
            }
            let entered = enter();
            pause.ask.store(false, Ordering::SeqCst);
            app.mouse_held = true;
            // A view that could not be printed is said; the session goes on.
            if let Err(err) = shown {
                app.say(format!("the scrollback could not be printed: {err}"));
            }
            entered?;
            execute!(io::stdout(), Clear(ClearType::All))?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            // `leave` gave the terminal its own cursor back: the vi mode's block is sent again.
            block_cursor = false;
            continue;
        }
        // Ctrl+X E: the editor gets the terminal, and the input comes back from it (T29).
        if let Some(text) = app.edit_request.take() {
            pause.ask.store(true, Ordering::SeqCst);
            // The reader says when it stopped reading (after its poll ends): only then has the editor the keys.
            while !pause.idle.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            leave(enhanced);
            let edited = edit_in_editor(&app.parts.home, &text);
            let entered = enter();
            pause.ask.store(false, Ordering::SeqCst);
            // The text is the owner's even when the terminal cannot be taken back.
            app.edited(edited);
            entered?;
            // Taking the terminal back takes the mouse too, whatever F2 had said.
            app.mouse_held = true;
            execute!(io::stdout(), Clear(ClearType::All))?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            // `leave` gave the terminal its own cursor back: the vi mode's block is sent again.
            block_cursor = false;
            continue;
        }
        tokio::select! {
            // A closed window or a kill ends the session cleanly: the turn stopped, the terminal given back.
            _ = terminate.recv() => app.abandon(),
            _ = hangup.recv() => app.abandon(),
            key = keys.recv() => match key {
                Some(key) => app.on_terminal(key),
                None => return Ok(()),
            },
            report = from_session.recv() => {
                if let Some(report) = report {
                    app.on_report(report);
                }
                // A stream of small pieces is drawn once, not once per piece.
                while let Ok(report) = from_session.try_recv() {
                    app.on_report(report);
                }
            }
            // The tick runs, and a frame is drawn, only while something moves: Glut works, or Glimm is in view.
            _ = tick.tick(), if app.animated() => app.frame = app.frame.wrapping_add(1),
        }
    }
}

/// The terminal's keys, pastes and clicks, read on a thread of their own and handed to the loop.
/// The reader thread's pause for the editor: the loop asks, the reader answers once it no longer reads.
#[derive(Default)]
struct Pause {
    ask: AtomicBool,
    idle: AtomicBool,
}

fn terminal_events(
    stop: Arc<AtomicBool>,
    pause: Arc<Pause>,
) -> mpsc::UnboundedReceiver<event::Event> {
    let (send, receive) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            // While the editor has the terminal, its keys are the editor's.
            if pause.ask.load(Ordering::SeqCst) {
                pause.idle.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            pause.idle.store(false, Ordering::SeqCst);
            match event::poll(POLL) {
                Ok(true) => match event::read() {
                    Ok(event) => {
                        if send.send(event).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                },
                Ok(false) => {}
                Err(_) => return,
            }
        }
    });
    receive
}

/// The input written in the owner's editor: into a file only the owner can read, the editor on it, the file read
/// back and removed. Without an editor set (and no `$VISUAL`/`$EDITOR`) it says how to set one.
fn edit_in_editor(home: &std::path::Path, text: &str) -> Result<String, String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    /// Removes the file whatever happens after it was made.
    struct Gone(std::path::PathBuf);

    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let template = prefs::Prefs::load(home)
        .editor()
        .ok_or("no editor yet — /editor picks one")?;
    // A folder only the owner can list: the name of the file is nobody else's to guess.
    let dir = home.join(glut::providers::DIR).join("tmp");
    glut::providers::make_private_dir(&dir)
        .map_err(|err| format!("cannot use {}: {err}", dir.display()))?;
    let file = dir.join(format!(
        "message-{}-{}.md",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
        .map_err(|err| format!("cannot write {}: {err}", file.display()))?;
    let _gone = Gone(file.clone());
    out.write_all(text.as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", file.display()))?;
    drop(out);
    let line = text.lines().count().max(1);
    let (program, args) = prefs::editor_command(&template, &file, line)
        .ok_or("the editor's command is empty — /editor picks one")?;
    match std::process::Command::new(&program).args(&args).status() {
        Ok(status) if status.success() => std::fs::read_to_string(&file)
            .map_err(|err| format!("cannot read {}: {err}", file.display())),
        Ok(status) => Err(format!(
            "{program} ended with {status}: the input stays as it was"
        )),
        Err(err) => Err(format!(
            "cannot start {program}: {err} — /editor picks another"
        )),
    }
}

/// Takes the terminal: raw mode, the alternate screen, pastes as one, the mouse, and — where the terminal has it —
/// the kitty keyboard protocol, so Shift+Enter is not Enter. Returns whether the protocol was pushed.
fn enter() -> io::Result<bool> {
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )?;
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    Ok(enhanced)
}

/// Gives the terminal back as it was.
fn leave(enhanced: bool) {
    let mut out = io::stdout();
    if enhanced {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        cursor::SetCursorStyle::DefaultUserShape,
        cursor::Show
    );
    let _ = terminal::disable_raw_mode();
    let _ = out.flush();
}
