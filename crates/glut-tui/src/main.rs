//! `glut` — Glut's terminal interface (`docs/plans/glut-tui.md`, TUI1a): one session in a folder, full screen, the
//! default provider of `~/.axh/providers.json` unless `--provider` names another.

mod app;
mod glimm;
mod input;
mod picker;
mod prefs;
mod session;
mod setup;
mod store;
mod theme;
mod transcript;
mod view;

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

use app::App;
use session::Start;

/// How often the screen moves on its own while Glut works or Glimm is in view (its flicker, its breath).
const TICK: Duration = Duration::from_millis(120);
/// How long the terminal thread waits for a key before it looks whether it should stop.
const POLL: Duration = Duration::from_millis(100);

#[derive(Parser)]
#[command(name = "glut", version, about = "Glut, in the terminal")]
struct Cli {
    /// The folder the session works in.
    #[arg(long, default_value = ".")]
    cwd: PathBuf,
    /// A provider of ~/.axh/providers.json (axh providers list); the default one otherwise.
    #[arg(long)]
    provider: Option<String>,
    /// The model; the provider's default otherwise.
    #[arg(long)]
    model: Option<String>,
    /// Go on with the newest session of the folder.
    #[arg(short = 'c', long = "continue", conflicts_with = "resume")]
    continue_last: bool,
    /// Pick an earlier session of the folder to go on with.
    #[arg(short = 'r', long)]
    resume: bool,
    /// A first message, sent as soon as the session is up.
    prompt: Option<String>,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let (reports, mut from_session) = mpsc::unbounded_channel();
    // The first start asks for a provider instead of ending with an error.
    if let Ok(home) = glut::permissions::settings::home_dir()
        && let Err(reason) = setup::first_provider(&home).await
    {
        eprintln!("glut: {reason}");
        return std::process::ExitCode::from(1);
    }
    if let Ok(home) = glut::permissions::settings::home_dir()
        && let Err(reason) =
            setup::missing_model(&home, cli.provider.as_deref(), cli.model.as_deref()).await
    {
        eprintln!("glut: {reason}");
        return std::process::ExitCode::from(1);
    }
    eprintln!("Glut is starting…");
    let (parts, engine) = match session::build(
        Start {
            cwd: cli.cwd,
            provider: cli.provider,
            model: cli.model,
        },
        reports.clone(),
    )
    .await
    {
        Ok((parts, engine)) => (Arc::new(parts), engine),
        Err(reason) => {
            eprintln!("glut: {reason}");
            return std::process::ExitCode::from(1);
        }
    };
    let mut app = App::new(parts, engine, reports);
    if cli.continue_last {
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
    if cli.resume {
        app.picker = Some(app.session_picker());
    }
    if let Some(prompt) = cli.prompt {
        app.send(prompt);
    }

    let enhanced = match enter() {
        Ok(enhanced) => enhanced,
        Err(err) => {
            leave(false);
            eprintln!("glut: cannot use this terminal: {err}");
            return std::process::ExitCode::from(1);
        }
    };
    // A panic of the screen's own thread must not leave the owner's terminal in raw mode on the alternate screen. A
    // turn's panic is caught where the turn ends (`session::turn`) and the screen goes on: it keeps the terminal.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().name() == Some("main") {
            leave(enhanced);
        }
        previous(info);
    }));

    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(Pause::default());
    let mut keys = terminal_events(Arc::clone(&stop), Arc::clone(&pause));
    let outcome = run(&mut app, &mut keys, &mut from_session, enhanced, &pause).await;
    stop.store(true, Ordering::Relaxed);
    leave(enhanced);
    let _ = std::panic::take_hook();
    match outcome {
        Ok(()) => {
            println!(
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
            eprintln!("glut: {err}");
            std::process::ExitCode::from(1)
        }
    }
}

/// The main loop: draws, then waits for a key, a step of the run or the next tick.
async fn run(
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
    loop {
        // Every frame inside synchronized output (DEC 2026): the terminal shows it whole or not at all — and the end
        // is sent even when the frame failed, or the terminal would hold every later one back.
        execute!(io::stdout(), BeginSynchronizedUpdate)?;
        let drawn = terminal.draw(|frame| view::draw(frame, app));
        execute!(io::stdout(), EndSynchronizedUpdate)?;
        drawn?;
        if app.quit {
            app.stop_turn();
            return Ok(());
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
            execute!(io::stdout(), Clear(ClearType::All))?;
            terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
            continue;
        }
        tokio::select! {
            // A closed window or a kill ends the session cleanly: the turn stopped, the terminal given back.
            _ = terminate.recv() => app.quit = true,
            _ = hangup.recv() => app.quit = true,
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
        cursor::Show
    );
    let _ = terminal::disable_raw_mode();
    let _ = out.flush();
}
