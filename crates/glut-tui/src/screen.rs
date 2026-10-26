//! The screen (TUI1a, made a library's in TUI8a): takes the terminal, runs one session on it until the owner ends
//! it, and gives the terminal back — for the standalone `glut` and for a host that runs Glut for its own ends.

use std::io::{self, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
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
use crate::panes::{Inlets, Panes, Switch};
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
    /// The kept session of this id to go on with (`glut --session`, a search's hit in another project, T55).
    pub session: Option<String>,
    /// What the stream's search looks for once that session is open (`--find`).
    pub find: Option<String>,
    /// A run of the host's that was cut off: it goes on first, from its journal ([`crate::Turn::Resumed`]), and the
    /// first message follows unless the run had it already.
    pub lost_turn: Option<String>,
}

/// The owner's own session, as the standalone `glut` runs it — also for a host that hands the owner Glut as it is, with
/// no part of its own (Axiomata's master pane, TUI8c): the first start asks for a provider and a missing model is
/// chosen before the screen comes up, then [`run`] with the [`Standalone`](crate::Standalone) host.
pub async fn standalone(launch: Launch, open: Open) -> std::process::ExitCode {
    standalone_switching(launch, open).await.0
}

/// [`standalone`], and the project the owner switched to (T44, `/projects`): the caller starts Glut anew there with
/// [`restart_in`] — once its runtime is gone, which takes the servers and shells this one started down with it.
pub async fn standalone_switching(
    launch: Launch,
    open: Open,
) -> (std::process::ExitCode, Option<Switch>) {
    if let Ok(home) = glut::permissions::settings::home_dir() {
        // The language read from the first line on: the setup and the start speak it too (T48).
        crate::texts::apply(
            crate::prefs::Prefs::load(&home).language.as_deref(),
            launch.language,
        );
        if let Err(reason) = setup::first_provider(&home).await {
            eprintln!("glut: {reason}");
            return (std::process::ExitCode::from(1), None);
        }
        if let EngineChoice::Providers { provider, model } = &launch.engine
            && let Err(reason) =
                setup::missing_model(&home, provider.as_deref(), model.as_deref()).await
        {
            eprintln!("glut: {reason}");
            return (std::process::ExitCode::from(1), None);
        }
    }
    run_switching(launch, Arc::new(crate::Standalone), open, |_| {}).await
}

/// Glut anew in `folder` (T44, `/projects`), as `glut -c` typed there: this process becomes it — its sandbox root,
/// rules, guides, servers and trust those of a start by hand there. Called once the runtime that ran this window is
/// gone: what it held — MCP servers, shells, the sandbox's proxy — ended with it, as at any end.
pub fn restart_in(to: &Switch) -> std::process::ExitCode {
    use std::os::unix::process::CommandExt as _;
    let folder = to.folder.as_path();
    let shown = crate::transcript::clean(&folder.display().to_string());
    let program = match std::env::current_exe() {
        Ok(program) => program,
        Err(err) => {
            eprintln!(
                "glut: {}",
                tr!(restart_failed, folder = shown, reason = err)
            );
            return std::process::ExitCode::from(1);
        }
    };
    let mut command = std::process::Command::new(program);
    command.current_dir(folder).args(restart_args(
        to,
        crate::motion::reader_forced(),
        crate::texts::forced(),
    ));
    // Only comes back when it could not start.
    let err = command.exec();
    eprintln!(
        "glut: {}",
        tr!(restart_failed, folder = shown, reason = err)
    );
    std::process::ExitCode::from(1)
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
    run_switching(launch, host, open, linked).await.0
}

/// [`run`], and the project the owner switched to (T44), which Glut starts anew in.
async fn run_switching(
    launch: Launch,
    host: Arc<dyn Host>,
    open: Open,
    linked: impl FnOnce(HostLink),
) -> (std::process::ExitCode, Option<Switch>) {
    let (reports, from_session) = mpsc::unbounded_channel();
    // A pane split off later starts the same servers, never more (`--strict-mcp-config` stays strict).
    let servers = launch.servers.clone();
    eprintln!("{}", tr!(starting));
    let (parts, engine) = match session::build(launch, Arc::clone(&host), reports.clone()).await {
        Ok((parts, engine)) => (Arc::new(parts), engine),
        Err(reason) => {
            eprintln!("glut: {reason}");
            host.state(State::Ended);
            return (std::process::ExitCode::from(1), None);
        }
    };
    linked(HostLink(reports.clone()));
    let mut app = App::new(parts, engine, reports);
    let offer_layout =
        open.continue_last && open.prompt.is_none() && !open.pick && open.lost_turn.is_none();
    if open.continue_last {
        match store::Session::list(&app.parts.home, &app.parts.config.cwd)
            .into_iter()
            .next()
        {
            Some(session) => app.open(session),
            None => app
                .transcript
                .note(tr!(no_earlier_session), transcript::Tone::Quiet),
        }
    }
    if let Some(id) = &open.session {
        app.open_kept(id, open.find.as_deref());
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
    let (mut panes, mut inlets) = Panes::new(app, from_session, host.may_split(), servers);
    // `glut -c` goes on with the folder's newest session, and offers the panes kept with it (TUI9e) — not when a
    // first message, a pick or a lost turn has the first word.
    if offer_layout {
        panes.offer_layout();
    }
    let outcome = screen(&mut panes, &mut inlets, &host).await;
    host.state(State::Ended);
    // A terminal that failed is no place to start Glut anew in.
    let switch = panes
        .switch_to
        .take()
        .filter(|_| outcome == std::process::ExitCode::SUCCESS);
    (outcome, switch)
}

/// The session on the terminal, from taking it to giving it back.
async fn screen(
    panes: &mut Panes,
    inlets: &mut Inlets,
    host: &Arc<dyn Host>,
) -> std::process::ExitCode {
    let enhanced = match enter() {
        Ok(enhanced) => enhanced,
        Err(err) => {
            leave(false);
            eprintln!("glut: {}", tr!(terminal_unusable, reason = err));
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
    let outcome = run_loop(panes, &mut keys, inlets, host, enhanced, &pause).await;
    stop.store(true, Ordering::Relaxed);
    leave(enhanced);
    let _ = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| previous(info)));
    // The session stays where the terminal keeps it, above the shell's next prompt — settled first (a turn stopped
    // by the quit shows as stopped), its last lines only: a terminal keeps a few thousand anyway. Printed after an
    // error too: the alternate screen it was on is gone.
    // A switch (T44) leaves no stream behind: Glut starts anew at once, in the other folder.
    let switching = panes.switch_to.is_some();
    let app = panes.active();
    if !app.welcoming() && !switching {
        app.transcript.settle();
        let width = terminal::size().map_or(80, |(width, _)| width);
        let lines = app.stream_at(width);
        let tail = &lines[lines.len().saturating_sub(EXIT_LINES)..];
        let _ = scrollback::print(&mut io::stdout(), tail);
    }
    match outcome {
        Ok(()) if switching => std::process::ExitCode::SUCCESS,
        Ok(()) => {
            // Written, not printed: a terminal that is gone (SIGHUP) must not turn the end into a panic. Among
            // panes, each session's own figures.
            let split = panes.split();
            for pane in &panes.list {
                let app = &pane.app;
                let which = if split {
                    format!(" ({})", tr!(pane_named, n = pane.number))
                } else {
                    String::new()
                };
                let cost = app
                    .cost
                    .map(|cost| format!(", {}", glut::lang::usd(cost, 4)))
                    .unwrap_or_default();
                let _ = writeln!(
                    io::stdout(),
                    "Glut: {}",
                    tr!(
                        session_ended,
                        which = which,
                        input = app.usage.input_tokens,
                        output = app.usage.output_tokens,
                        cost = cost
                    )
                );
            }
            std::process::ExitCode::SUCCESS
        }
        Err(err) => {
            let _ = writeln!(io::stderr(), "glut: {err}");
            std::process::ExitCode::from(1)
        }
    }
}

/// What Glut is started with in `folder` (T44): the folder and `--continue` — nothing of this run's own flags (its
/// provider, model or `--mcp-config` were for the folder it was started in) but the reader mode the owner holds on.
fn restart_args(
    to: &Switch,
    reader: bool,
    lang: Option<glut::lang::Lang>,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec!["--cwd".into(), to.folder.as_os_str().into()];
    // A session a search found (T55) opens there, its stream searched; else the folder's newest goes on.
    match &to.session {
        Some(id) => {
            // One argument each: a text that starts with `-` is no flag of its own.
            args.push(format!("--session={id}").into());
            if let Some(find) = to.find.as_ref().filter(|find| !find.is_empty()) {
                args.push(format!("--find={find}").into());
            }
        }
        None => args.push("--continue".into()),
    }
    if reader {
        args.push("--reader".into());
    }
    // A language given for the run (T48) holds in the next folder too.
    if let Some(lang) = lang {
        args.extend(["--lang".into(), lang.code().into()]);
    }
    args
}

/// The main loop: draws, then waits for a key, a step of the run or the next tick.
async fn run_loop(
    panes: &mut Panes,
    keys: &mut mpsc::UnboundedReceiver<event::Event>,
    inlets: &mut Inlets,
    host: &Arc<dyn Host>,
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
    // Held still (TUI9b), a frame a second: the seconds of "working" go on, nothing else moves.
    let mut second = tokio::time::interval(Duration::from_secs(1));
    second.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    // Raw mode sends no SIGINT; at the scrollback's prompt (cooked mode) Ctrl+C would end Glut mid-turn: it goes
    // back instead.
    let mut interrupt = signal(SignalKind::interrupt())?;
    // The cursor's shape follows the vi mode: a block in Normal mode, the owner's own shape otherwise.
    let mut block_cursor = false;
    // The pictures the terminal draws itself (T46b): what it was sent and where each stands.
    let mut graphics = crate::graphics::Screen::default();
    // Pictures are decoded beside the screen (T47): each one ready has the frame drawn anew.
    let (picture_ready, mut pictures_ready) = mpsc::unbounded_channel();
    let _beside = crate::pictures::Beside::begin(picture_ready);
    loop {
        // A session built beside the screen may have written to the terminal: drawn anew from blank.
        if std::mem::take(&mut panes.redraw) {
            execute!(io::stdout(), Clear(ClearType::All))?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            graphics.cleared(&mut io::stdout())?;
        }
        let wanted = panes
            .active()
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
        panes.sync();
        // Every frame inside synchronized output (DEC 2026): the terminal shows it whole or not at all — and the end
        // is sent even when the frame failed, or the terminal would hold every later one back.
        execute!(io::stdout(), BeginSynchronizedUpdate)?;
        let mut spots = Vec::new();
        let mut pictures = Vec::new();
        let drawn = terminal.draw(|frame| {
            view::draw_panes(frame, panes);
            // The pictures' marks give up where they stand (T46b) — once the frame is drawn.
            pictures = crate::graphics::take(frame.buffer_mut());
            // The reader mode's frames, marks and Powerline's glyphs in plain ASCII, one cell for one (TUI9b) — before
            // the links take their cells, which are written once more as they stand then.
            crate::motion::plain_cells(frame.buffer_mut());
            // The links' marks give up where they stand before the frame goes out (TUI9d).
            spots = crate::links::take_spots(frame.buffer_mut());
        });
        // The links' cells once more, inside OSC 8 — in the same synchronized frame.
        let linked = match &drawn {
            Ok(_) => crate::links::write(&mut io::stdout(), &spots),
            Err(_) => Ok(()),
        };
        // The pictures the terminal draws itself, where they stand whole — in the same synchronized frame.
        let protocol = crate::graphics::protocol()
            .filter(|_| crate::pictures::drawing() == crate::pictures::Drawing::Inline);
        let shown = match (&drawn, protocol) {
            (Ok(_), Some(protocol)) => graphics.show(&mut io::stdout(), protocol, &pictures),
            // Drawn otherwise now (`/images`, the reader mode): what the terminal still draws is taken away.
            (Ok(_), None) if graphics.holds_any() => {
                graphics.cleared(&mut io::stdout()).map(|()| false)
            }
            _ => Ok(false),
        };
        execute!(io::stdout(), EndSynchronizedUpdate)?;
        drawn?;
        linked?;
        panes.links = spots;
        // iTerm2 keeps a picture in its cells: one moved or gone has the screen drawn anew, at once.
        if shown? && !panes.quitting() {
            panes.redraw = true;
            continue;
        }
        if panes.quitting() {
            graphics.cleared(&mut io::stdout())?;
            // How the window stood, for the next `glut -c` (TUI9e).
            panes.keep_layout();
            for pane in &mut panes.list {
                let app = &mut pane.app;
                // A turn the host can go on with later is left to its journal when the terminal is gone.
                if !(app.abandoned && app.parts.keep_lost_turns) {
                    app.stop_turn();
                }
            }
            return Ok(());
        }
        // F2: the mouse to the terminal (its own selection) and back — the screen's, so every pane's.
        if let Some(held) = panes.active().mouse_request.take() {
            if held {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
            for pane in &mut panes.list {
                pane.app.mouse_held = held;
            }
            panes.active().mouse_said(held);
        }
        let app = panes.active();
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
                print!("\n— Glut: {} ", tr!(scrollback_shown));
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
                app.say(tr!(scrollback_failed, reason = err));
            }
            entered?;
            execute!(io::stdout(), Clear(ClearType::All))?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            graphics.cleared(&mut io::stdout())?;
            // `leave` gave the terminal its own cursor back: the vi mode's block is sent again.
            block_cursor = false;
            continue;
        }
        // `/memory open`: the editor gets the terminal for a file of the owner's, and gives it back (T35).
        if let Some(file) = app.open_request.take() {
            pause.ask.store(true, Ordering::SeqCst);
            while !pause.idle.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            leave(enhanced);
            let opened = open_in_editor(&app.parts.home, &file);
            let entered = enter();
            pause.ask.store(false, Ordering::SeqCst);
            app.opened(opened);
            entered?;
            app.mouse_held = true;
            execute!(io::stdout(), Clear(ClearType::All))?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            graphics.cleared(&mut io::stdout())?;
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
            graphics.cleared(&mut io::stdout())?;
            // `leave` gave the terminal its own cursor back: the vi mode's block is sent again.
            block_cursor = false;
            continue;
        }
        let still = crate::motion::still();
        let animated = !still && panes.list.iter().any(|pane| pane.app.animated());
        // Held still, a second's frame is enough for what changes on its own: the seconds, a toast going.
        let counting = still
            && panes
                .list
                .iter()
                .any(|pane| pane.app.working() || pane.app.toast().is_some());
        // A notification gathering what comes in its five seconds goes once they are over (T53).
        let notify_due = panes.notifier.due();
        let notify_at =
            tokio::time::Instant::from_std(notify_due.unwrap_or_else(std::time::Instant::now));
        // A loop's next round starts at its time, whatever else moves (T60a).
        let loop_due = panes.loop_due();
        let loop_at =
            tokio::time::Instant::from_std(loop_due.unwrap_or_else(std::time::Instant::now));
        tokio::select! {
            // A closed window or a kill ends the session cleanly: the turn stopped, the terminal given back.
            _ = terminate.recv() => panes.list.iter_mut().for_each(|pane| pane.app.abandon()),
            _ = hangup.recv() => panes.list.iter_mut().for_each(|pane| pane.app.abandon()),
            key = keys.recv() => match key {
                Some(key) => panes.on_terminal(key),
                None => return Ok(()),
            },
            report = inlets.reports.recv() => {
                if let Some((session, report)) = report {
                    panes.on_report(session, report);
                }
                // A stream of small pieces is drawn once, not once per piece.
                while let Ok((session, report)) = inlets.reports.try_recv() {
                    panes.on_report(session, report);
                }
            }
            Some(()) = pictures_ready.recv() => {
                while pictures_ready.try_recv().is_ok() {}
                let fresh = crate::pictures::fresh();
                for pane in &mut panes.list {
                    pane.app.transcript.touch_pictures(&fresh);
                }
            }
            built = inlets.built.recv() => {
                if let Some(built) = built {
                    panes.on_built(built);
                }
            }
            // The tick runs, and a frame is drawn, only while something moves: Glut works, or Glimm is in view.
            _ = tick.tick(), if animated => {
                for pane in &mut panes.list {
                    pane.app.frame = pane.app.frame.wrapping_add(1);
                }
            }
            _ = second.tick(), if counting => {}
            _ = tokio::time::sleep_until(notify_at), if notify_due.is_some() => {}
            _ = tokio::time::sleep_until(loop_at), if loop_due.is_some() => {}
        }
        // What a pane asked for — a split, a close, the other pane — once its key or command is through.
        panes.settle(host);
        panes.send_notifications(&mut io::stdout());
    }
}

/// The terminal's keys, pastes and clicks, read on a thread of their own and handed to the loop.
/// The reader thread's pause for the editor: the loop asks, the reader answers once it no longer reads.
#[derive(Default)]
pub(crate) struct Pause {
    ask: AtomicBool,
    idle: AtomicBool,
}

pub(crate) fn terminal_events(
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
/// `file` in the owner's editor, until it ends.
fn open_in_editor(home: &std::path::Path, file: &std::path::Path) -> Result<(), String> {
    let template = prefs::Prefs::load(home).editor().ok_or(tr!(no_editor))?;
    let (program, args) = prefs::editor_command(&template, file, 1).ok_or(tr!(editor_empty))?;
    match std::process::Command::new(&program).args(&args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(tr!(editor_ended, program = program, status = status)),
        Err(err) => Err(tr!(editor_unstartable, program = program, reason = err)),
    }
}

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

    let template = prefs::Prefs::load(home).editor().ok_or(tr!(no_editor))?;
    // A folder only the owner can list: the name of the file is nobody else's to guess.
    let dir = home.join(glut::providers::DIR).join("tmp");
    glut::providers::make_private_dir(&dir)
        .map_err(|err| tr!(cannot_use, path = dir.display(), reason = err))?;
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
        .map_err(|err| tr!(cannot_write, path = file.display(), reason = err))?;
    let _gone = Gone(file.clone());
    out.write_all(text.as_bytes())
        .map_err(|err| tr!(cannot_write, path = file.display(), reason = err))?;
    drop(out);
    let line = text.lines().count().max(1);
    let (program, args) = prefs::editor_command(&template, &file, line).ok_or(tr!(editor_empty))?;
    match std::process::Command::new(&program).args(&args).status() {
        Ok(status) if status.success() => std::fs::read_to_string(&file)
            .map_err(|err| tr!(cannot_read_path, path = file.display(), reason = err)),
        Ok(status) => Err(tr!(editor_ended_input, program = program, status = status)),
        Err(err) => Err(tr!(editor_unstartable, program = program, reason = err)),
    }
}

/// Takes the terminal: raw mode, the alternate screen, pastes as one, the mouse, and — where the terminal has it —
/// the kitty keyboard protocol, so Shift+Enter is not Enter. Returns whether the protocol was pushed.
pub(crate) fn enter() -> io::Result<bool> {
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture,
        // The window's focus, for the notifications (T53): a terminal that has no reports ignores it.
        EnableFocusChange
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
pub(crate) fn leave(enhanced: bool) {
    let mut out = io::stdout();
    if enhanced {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        out,
        DisableMouseCapture,
        DisableFocusChange,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        cursor::SetCursorStyle::DefaultUserShape,
        cursor::Show
    );
    let _ = terminal::disable_raw_mode();
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_starts_glut_in_the_folder_as_glut_c_typed_there_and_keeps_only_the_reader_mode() {
        let folder = &Switch::to("/projects/other".into());
        assert_eq!(
            restart_args(folder, false, None),
            ["--cwd", "/projects/other", "--continue"]
        );
        // `main.rs` parses these very flags (its test `a_switch_s_restart_is_a_start_by_hand`).
        assert_eq!(restart_args(folder, true, None).last().unwrap(), "--reader");
        assert_eq!(
            restart_args(folder, false, Some(glut::lang::Lang::Ja))[3..],
            ["--lang", "ja"]
        );
        // A session found in another project (T55) opens there, its stream searched.
        let found = Switch {
            session: Some("20261025-1".into()),
            find: Some("-rf sandbox".into()),
            ..folder.clone()
        };
        assert_eq!(
            restart_args(&found, false, None),
            [
                "--cwd",
                "/projects/other",
                "--session=20261025-1",
                "--find=-rf sandbox"
            ]
        );
    }
}
