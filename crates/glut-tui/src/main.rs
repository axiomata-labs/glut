//! `glut` — Glut's terminal interface (`docs/plans/glut-tui.md`, TUI1a): one session in a folder, full screen, the
//! default provider of `~/.axh/providers.json` unless `--provider` names another.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
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
    /// A provider of ~/.axh/providers.json (axh providers list); the default one otherwise.
    #[arg(long)]
    provider: Option<String>,
    /// The model; the provider's default otherwise.
    #[arg(long)]
    model: Option<String>,
    /// Go on with the newest session of the folder.
    #[arg(short = 'c', long = "continue", conflicts_with = "resume")]
    continue_last: bool,
    /// MCP servers from this file (Claude Code's shape and flag, `{"mcpServers": …}`) beside `~/.axh/mcp.json` and the
    /// project's `.mcp.json`; a name both have is the file's.
    #[arg(long, value_name = "FILE")]
    mcp_config: Option<PathBuf>,
    /// The servers of `--mcp-config` alone, none of the owner's nor the project's — `axh run --mcp-config`'s meaning.
    #[arg(long, requires = "mcp_config")]
    strict_mcp_config: bool,
    /// Pick an earlier session of the folder to go on with.
    #[arg(short = 'r', long)]
    resume: bool,
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
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    if let Some(Command::Watch { what }) = &cli.command {
        return watch(&cli.cwd, what.as_deref()).await;
    }
    glut_tui::standalone(launch(&cli), open(cli)).await
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
        prompt: cli.prompt,
        ..Open::default()
    }
}

#[cfg(test)]
mod tests {
    use glut_tui::EngineChoice;

    use super::*;

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
