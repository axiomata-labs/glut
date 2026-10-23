//! `glut` — Glut's terminal interface (`docs/plans/glut-tui.md`, TUI1a): one session in a folder, full screen, the
//! default provider of `~/.axh/providers.json` unless `--provider` names another.

use std::path::PathBuf;

use clap::Parser;
use glut_tui::{Launch, Open, Servers};

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

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    glut_tui::standalone(launch(&cli), open(cli)).await
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
    }
}
