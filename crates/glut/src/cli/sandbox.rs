//! `glut sandbox` (T59): how the commands Glut runs are held — the same as `/sandbox` in the terminal UI. A command
//! Glut runs is refused every change: the switch is the owner's word alone.

use std::process::ExitCode;

use clap::{Subcommand, ValueEnum};

use crate::sandbox::choices::Choices;

/// What `glut sandbox` does.
#[derive(Subcommand)]
pub enum SandboxCommand {
    /// Shows the sandbox here and whether git folders are read-only to the commands Glut runs.
    Show,
    /// Sets whether the commands Glut runs may write git folders (read-only: none commits from the sandbox).
    Git {
        /// read-only or writable.
        #[arg(value_enum)]
        mode: GitMode,
    },
    /// Sets whether the commands Glut runs may run without a sandbox (on: none does where there is none).
    Required {
        /// on or off.
        #[arg(value_enum)]
        mode: OnOff,
    },
}

/// A switch's two ways.
#[derive(Clone, Copy, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

/// What the commands Glut runs may do with git folders.
#[derive(Clone, Copy, ValueEnum)]
pub enum GitMode {
    // No command writes a git folder (not a doc comment: clap would show it untranslated in the help).
    ReadOnly,
    // Commands may write git folders, commits too.
    Writable,
}

/// Runs one `glut sandbox` command.
pub fn run(command: SandboxCommand) -> ExitCode {
    let home = match crate::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    match command {
        SandboxCommand::Show => {
            // The owner's settings may have switched it off: `sandbox.enabled`.
            let enabled = std::env::current_dir()
                .ok()
                .and_then(|cwd| crate::permissions::settings::load(&home, &cwd, false).ok())
                .is_none_or(|loaded| loaded.sandbox.enabled);
            match crate::sandbox::unavailable() {
                _ if !enabled => println!("{}", say!(sandbox_off)),
                Some(why) => println!("{}", say!(sandbox_missing, why = why)),
                None => println!("{}", say!(sandbox_on, kind = crate::sandbox::mechanism())),
            }
            match Choices::load(&home) {
                Ok(choices) if choices.git_read_only => println!("{}", say!(sandbox_git_read_only)),
                Ok(_) => println!("{}", say!(sandbox_git_writable)),
                Err(err) => println!("{}", say!(sandbox_unreadable, reason = err.to_string())),
            }
            if Choices::in_force(&home).required {
                println!("{}", say!(sandbox_required_shown));
            }
            ExitCode::SUCCESS
        }
        SandboxCommand::Git { .. } | SandboxCommand::Required { .. }
            if std::env::var_os(crate::tools::shell::SANDBOXED_ENV).is_some() =>
        {
            fail(say!(sandbox_owners_only))
        }
        SandboxCommand::Git { mode } => {
            // The other switch stays as it stands.
            let mut choices = Choices::in_force(&home);
            choices.git_read_only = matches!(mode, GitMode::ReadOnly);
            save(
                &home,
                choices,
                if choices.git_read_only {
                    say!(sandbox_git_now_read_only)
                } else {
                    say!(sandbox_git_now_writable)
                },
            )
        }
        SandboxCommand::Required { mode } => {
            let mut choices = Choices::in_force(&home);
            choices.required = matches!(mode, OnOff::On);
            save(
                &home,
                choices,
                if choices.required {
                    say!(sandbox_required_now_on)
                } else {
                    say!(sandbox_required_now_off)
                },
            )
        }
    }
}

/// Keeps `choices` and says `done`.
fn save(home: &std::path::Path, choices: Choices, done: &str) -> ExitCode {
    match choices.save(home) {
        Ok(()) => {
            println!("{done}");
            ExitCode::SUCCESS
        }
        Err(err) => fail(&err.to_string()),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("glut sandbox: {message}");
    ExitCode::from(1)
}
