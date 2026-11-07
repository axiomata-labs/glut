//! Glut's own build tasks, run from the repository's top as `cargo run -p xtask -- <task>`. Nothing here is shipped:
//! the program `glut` never depends on it.

mod record;
mod site;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Glut's build tasks.
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// The website: built from the repository's Markdown and `site/`, into `target/site`.
    Site {
        #[command(subcommand)]
        what: SiteTask,
    },
    /// Records a scene of `site/recordings/<name>.scene`: Glut run for real in a pseudo-terminal, its screens kept in
    /// `site/recordings/<name>.json` for the site's player.
    Record {
        /// The scene's name.
        name: String,
    },
}

#[derive(Subcommand)]
enum SiteTask {
    /// Builds every page into the output folder.
    Build {
        /// Where the pages go.
        #[arg(long, default_value = "target/site")]
        out: PathBuf,
    },
    /// Builds, then checks that every link and anchor between the pages leads somewhere.
    Check {
        #[arg(long, default_value = "target/site")]
        out: PathBuf,
    },
    /// Builds and serves the pages on this machine alone, building again when a source changed.
    Serve {
        #[arg(long, default_value = "target/site")]
        out: PathBuf,
        /// The port on 127.0.0.1.
        #[arg(long, default_value_t = 8000)]
        port: u16,
    },
    /// Builds, checks, then replaces the site on its web server (glut-ai.com) over SSH.
    Deploy {
        #[arg(long, default_value = "target/site")]
        out: PathBuf,
        /// The SSH key that lets the deploy in; the owner's `~/.config/glut-ai/deploy_ed25519` where none is named.
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let args = Args::parse();
    let repo = match repository() {
        Ok(repo) => repo,
        Err(err) => return fail(&err),
    };
    let done = match args.task {
        Task::Site { what } => match what {
            SiteTask::Build { out } => site::build(&repo, &repo.join(out)).map(|count| {
                println!("site: {count} pages");
            }),
            SiteTask::Check { out } => site::check(&repo, &repo.join(out)),
            SiteTask::Serve { out, port } => site::serve(&repo, &repo.join(out), port),
            SiteTask::Deploy { out, key } => site::deploy(&repo, &repo.join(out), key),
        },
        Task::Record { name } => record::record(&repo, &name),
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => fail(&err),
    }
}

/// The repository's top: this crate lives in `crates/xtask`, wherever cargo was started.
fn repository() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .map(PathBuf::from)
        .ok_or_else(|| format!("no repository above {}", manifest.display()))
}

fn fail(message: &str) -> ExitCode {
    eprintln!("xtask: {message}");
    ExitCode::from(1)
}
