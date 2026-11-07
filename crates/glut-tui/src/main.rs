//! `glut` — Glut's terminal interface (TUI1a): one session in a folder, full screen, the
//! default provider of `~/.glut/providers.json` unless `--provider` names another.

use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use glut_tui::{Launch, Open, Servers};

#[derive(Parser)]
#[command(
    name = "glut",
    version,
    about = "Glut, in the terminal",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// The folder the session works in.
    #[arg(long, default_value = ".")]
    cwd: PathBuf,
    /// A provider of ~/.glut/providers.json (glut providers list); the default one otherwise.
    #[arg(long)]
    provider: Option<String>,
    /// The model; the provider's default otherwise.
    #[arg(long)]
    model: Option<String>,
    /// Go on with the newest session of the folder.
    #[arg(short = 'c', long = "continue", conflicts_with = "resume")]
    continue_last: bool,
    /// MCP servers from this file (Claude Code's shape and flag, `{"mcpServers": …}`) beside `~/.glut/mcp.json` and the
    /// project's `.mcp.json`; a name both have is the file's.
    #[arg(long, value_name = "FILE")]
    mcp_config: Option<PathBuf>,
    /// The servers of `--mcp-config` alone, none of the owner's nor the project's — `glut run --mcp-config`'s meaning.
    #[arg(long, requires = "mcp_config")]
    strict_mcp_config: bool,
    /// The reader mode for this run, for a screen reader: keys in words, a plain screen, nothing moving.
    #[arg(long)]
    reader: bool,
    /// The language Glut speaks for this run: en, de, fr, it, es, zh, ja or pt.
    #[arg(long, global = true, value_parser = parse_lang)]
    lang: Option<glut::lang::Lang>,
    /// Pick an earlier session of the folder to go on with.
    #[arg(short = 'r', long)]
    resume: bool,
    /// Go on with the kept session of this id (glut search finds it).
    #[arg(long, value_name = "ID", conflicts_with_all = ["resume", "continue_last"])]
    session: Option<String>,
    /// With --session: the stream searched for this text once it is open.
    #[arg(long, value_name = "TEXT", requires = "session")]
    find: Option<String>,
    /// A first message, sent as soon as the session is up.
    prompt: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Follow another session read-only from its event log: the folder's newest session, a session's id, or a log
    /// file (a Studio card's glut-events.jsonl).
    Watch {
        /// A session's id or a log file; the newest session of the folder when left out.
        what: Option<String>,
    },
    /// Write a session out to read or share: a session's id or file, or a run's id or journal; Markdown, HTML or JSON
    /// lines, on stdout unless --out names a new file.
    Export {
        /// A session's id or its file (~/.glut/sessions/<id>.json), or a run's id or journal (~/.glut/runs/<id>.jsonl).
        what: String,
        /// md, html or jsonl.
        #[arg(long, default_value = "md", value_parser = parse_format)]
        format: glut_tui::ExportFormat,
        /// The whole results from the session's store and the system prompt too, not only what the model saw.
        #[arg(long)]
        full: bool,
        /// A new file to write it to (never one that is there).
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Find kept sessions whose title or conversation holds every word, newest first: a line each with its id, its
    /// title and where it was said.
    Search {
        /// The words to find, in any order and any case.
        #[arg(required = true)]
        words: Vec<String>,
        /// Every project's sessions, not only the folder's.
        #[arg(long)]
        all: bool,
    },
    /// Plans that run while Glut is closed: at a time of day, or every so often.
    Schedule {
        #[command(subcommand)]
        what: glut_tui::ScheduleCommand,
    },
    /// Cleans Glut's own data in ~/.glut by the owner's terms: old snapshots, journals and sessions, and what only they
    /// held.
    Clean {
        #[command(subcommand)]
        what: Option<glut_tui::CleanCommand>,
        /// Counts what would go and removes nothing.
        #[arg(long)]
        dry_run: bool,
        /// The report as JSON, for a program to read.
        #[arg(long)]
        json: bool,
    },
    /// Serve an editor over the Agent Client Protocol on stdin and stdout (Zed, JetBrains, Neovim, Emacs): the
    /// owner's own Glut session in the folder the editor names, its gate asking through the editor.
    Acp {
        /// Set a provider up in this terminal as Glut's first start does, then end — what an editor runs to sign in.
        #[arg(long)]
        setup: bool,
    },
    #[command(flatten)]
    Headless(glut::cli::Command),
}

/// `--format`'s word.
fn parse_format(word: &str) -> Result<glut_tui::ExportFormat, String> {
    glut_tui::ExportFormat::parse(word).ok_or_else(|| "one of md, html, jsonl".to_owned())
}

/// The language `--lang` names among `args`, before clap reads them: `--lang de` or `--lang=de`.
fn lang_given(args: impl Iterator<Item = String>) -> Option<glut::lang::Lang> {
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        // Past `--` everything is the prompt's.
        if arg == "--" {
            break;
        }
        let word = match arg.strip_prefix("--lang") {
            Some("") => args.next()?,
            Some(rest) => match rest.strip_prefix('=') {
                Some(word) => word.to_owned(),
                // Another flag that starts alike (`--language`): not this one, the scan goes on.
                None => continue,
            },
            None => continue,
        };
        return glut::lang::Lang::parse(&word);
    }
    None
}

/// `--lang`'s word: a language Glut speaks.
fn parse_lang(word: &str) -> Result<glut::lang::Lang, String> {
    glut::lang::Lang::parse(word).ok_or_else(|| "one of en, de, fr, it, es, zh, ja, pt".to_owned())
}

/// How long a `glut run` that ended waits for its pass over `~/.glut` (T82) before it exits.
const CLEAN_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long the runtime's last tasks get to end before a switch starts Glut anew (T44).
const SHUTDOWN: std::time::Duration = std::time::Duration::from_secs(3);

fn main() -> std::process::ExitCode {
    // The Linux sandbox starts this program again inside it as its helper (T59): before anything else, no thread yet.
    if let Some(code) = glut::sandbox::serve_helper() {
        return std::process::ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // The help speaks the language the run will (T48): `--lang` is read before clap reads the rest.
    if let Some(lang) = lang_given(std::env::args()) {
        glut_tui::force_language(lang);
    }
    glut_tui::apply_kept_language();
    let matches = glut_tui::localized_cli(Cli::command()).get_matches();
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("glut: cannot start: {err}");
            return std::process::ExitCode::from(1);
        }
    };
    let (code, switch) = runtime.block_on(async {
        match cli.command.take() {
            Some(Command::Watch { what }) => (watch(&cli.cwd, what.as_deref()).await, None),
            Some(Command::Export {
                what,
                format,
                full,
                out,
            }) => (
                glut_tui::export_headless(&what, format, full, out.as_deref()),
                None,
            ),
            Some(Command::Search { words, all }) => {
                (glut_tui::search_headless(&words, all, &cli.cwd), None)
            }
            Some(Command::Schedule { what }) => (glut_tui::schedule_headless(what).await, None),
            Some(Command::Acp { setup }) => (glut_tui::acp_serve(setup).await, None),
            Some(Command::Clean {
                what,
                dry_run,
                json,
            }) => (glut_tui::clean_headless(what, dry_run, json), None),
            Some(Command::Headless(command)) => {
                // T82: a run cleans `~/.glut` once a day as Glut does at its start, and says on stderr what went.
                let cleaning = if matches!(command, glut::cli::Command::Run(_))
                    && let Ok(home) = glut::permissions::settings::home_dir()
                {
                    glut_tui::clean_auto(&home, |report| {
                        if let Some(note) = glut_tui::clean_freed(report) {
                            eprintln!("glut: {note}");
                        }
                    })
                } else {
                    None
                };
                let code = glut::cli::dispatch(command).await;
                // A short run waits a little for its pass, so it is not cut off halfway (the next start finishes it).
                if let Some(cleaning) = cleaning {
                    let until = std::time::Instant::now() + CLEAN_WAIT;
                    while !cleaning.is_finished() && std::time::Instant::now() < until {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
                (code, None)
            }
            None => {
                if cli.reader {
                    glut_tui::motion::force_reader();
                }
                glut_tui::standalone_switching(launch(&cli), open(cli)).await
            }
        }
    });
    match switch {
        // T44: the window's servers, shells and proxy end with the runtime — before Glut starts anew in the other
        // folder, a hung one not holding the switch up.
        Some(to) => {
            runtime.shutdown_timeout(SHUTDOWN);
            glut_tui::restart_in(&to)
        }
        None => code,
    }
}

/// `glut watch`: the log `what` names, followed until the owner quits.
async fn watch(cwd: &std::path::Path, what: Option<&str>) -> std::process::ExitCode {
    let found = glut::permissions::settings::home_dir()
        .map_err(|err| err.to_string())
        .and_then(|home| {
            // Sessions are kept by their folder's full path, as a session started here names it.
            let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
            glut_tui::watched_log(&home, &cwd, what).map(|log| (home, log))
        });
    match found {
        Ok((home, log)) => glut_tui::watch(home, log).await,
        Err(reason) => {
            eprintln!("glut: {reason}");
            std::process::ExitCode::from(1)
        }
    }
}

/// The session `cli` asks for: the owner's own, with the servers of `--mcp-config` beside theirs or, strict, alone.
fn launch(cli: &Cli) -> Launch {
    let mut launch = Launch::owner(cli.cwd.clone(), cli.provider.clone(), cli.model.clone());
    if let Some(file) = &cli.mcp_config {
        launch.servers = if cli.strict_mcp_config {
            Servers::Only(file.clone())
        } else {
            Servers::Also(file.clone())
        };
    }
    launch
}

/// How the session `cli` asks for opens.
fn open(cli: Cli) -> Open {
    Open {
        continue_last: cli.continue_last,
        pick: cli.resume,
        session: cli.session,
        find: cli.find,
        prompt: cli.prompt,
        ..Open::default()
    }
}

#[cfg(test)]
mod tests {
    use glut_tui::EngineChoice;

    use super::*;

    #[test]
    fn every_help_text_is_in_every_language() {
        let mut cmd = Cli::command();
        cmd.build();
        for lang in glut::lang::Lang::ALL
            .into_iter()
            .filter(|lang| *lang != glut::lang::Lang::En)
        {
            let missing = glut_tui::untranslated_cli(&cmd, lang);
            assert!(missing.is_empty(), "{}: {missing:#?}", lang.code());
        }
    }

    #[test]
    fn lang_is_read_before_clap_and_not_from_the_prompt() {
        let given = |args: &[&str]| lang_given(args.iter().map(|arg| (*arg).to_owned()));
        assert_eq!(
            given(&["glut", "--lang", "de", "--help"]),
            Some(glut::lang::Lang::De)
        );
        assert_eq!(
            given(&["glut", "run", "--lang=ja"]),
            Some(glut::lang::Lang::Ja)
        );
        assert_eq!(given(&["glut", "--", "--lang", "fr"]), None);
        assert_eq!(given(&["glut", "--language", "fr"]), None);
        assert_eq!(
            given(&["glut", "--language", "fr", "--lang", "it"]),
            Some(glut::lang::Lang::It)
        );
        assert_eq!(given(&["glut"]), None);
    }

    #[test]
    fn a_switch_s_restart_is_a_start_by_hand() {
        // What `/projects` starts Glut anew with (`screen::restart_args`, T44): the owner's own session there.
        let cli = Cli::parse_from(["glut", "--cwd", "/projects/other", "--continue", "--reader"]);
        assert!(cli.continue_last && cli.reader && !cli.resume && cli.prompt.is_none());
        assert_eq!(cli.cwd, std::path::Path::new("/projects/other"));
        let launch = launch(&cli);
        assert!(launch.owner_trust && launch.projects && matches!(launch.servers, Servers::Owner));
        // A session a search found in that project (T55), its stream searched.
        let cli = Cli::parse_from([
            "glut",
            "--cwd",
            "/p",
            "--session=s-1",
            "--find=-rf two words",
        ]);
        let open = open(cli);
        assert_eq!(
            (open.session.as_deref(), open.find.as_deref()),
            (Some("s-1"), Some("-rf two words"))
        );
        assert!(!open.continue_last && !open.pick);
        assert!(
            Cli::try_parse_from(["glut", "--find", "x"]).is_err(),
            "--find needs --session"
        );
    }

    #[test]
    fn mcp_config_adds_its_servers_and_strict_takes_them_alone() {
        let cli = Cli::parse_from(["glut", "--mcp-config", "/run/team.json", "--model", "m"]);
        let named = launch(&cli);
        assert!(
            matches!(&named.servers, Servers::Also(file) if file == std::path::Path::new("/run/team.json"))
        );
        assert!(named.owner_trust && named.keep_sessions && named.owner_shell);
        assert!(
            matches!(&named.engine, EngineChoice::Providers { model: Some(m), .. } if m == "m")
        );
        assert!(matches!(
            launch(&Cli::parse_from(["glut"])).servers,
            Servers::Owner
        ));
        let strict = Cli::parse_from([
            "glut",
            "--mcp-config",
            "/run/team.json",
            "--strict-mcp-config",
        ]);
        assert!(matches!(launch(&strict).servers, Servers::Only(_)));
        assert!(
            Cli::try_parse_from(["glut", "--strict-mcp-config"]).is_err(),
            "strict names no file"
        );
        assert!(matches!(
            Cli::parse_from(["glut", "watch", "s1"]).command,
            Some(Command::Watch { what: Some(id) }) if id == "s1"
        ));
        assert!(
            Cli::parse_from(["glut", "fix the parser"])
                .command
                .is_none()
        );
    }
}
