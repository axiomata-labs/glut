//! `glut` — Glut's terminal interface (`docs/plans/glut-tui.md`, TUI1a): one session in a folder, full screen, the
//! default provider of `~/.axh/providers.json` unless `--provider` names another.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use glut_tui::{Launch, Open, Standalone, setup};

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
    let open = Open {
        continue_last: cli.continue_last,
        pick: cli.resume,
        prompt: cli.prompt,
        ..Open::default()
    };
    glut_tui::run(
        Launch::owner(cli.cwd, cli.provider, cli.model),
        Arc::new(Standalone),
        open,
        |_| {},
    )
    .await
}
