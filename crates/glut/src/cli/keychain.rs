//! `glut keychain` (T74): the Keychain for the commands Glut runs — shut to every one but the programs on the owner's
//! list, which ask for it each time — the same as `/keychain` in the terminal UI, but what it opens stays open across
//! sessions. A command Glut runs is refused every change: the list is the owner's word alone.

use std::process::ExitCode;

use clap::Subcommand;

use crate::keychain::Keychain;

#[derive(Subcommand)]
pub enum KeychainCommand {
    /// Shows whether the Keychain is open to the commands Glut runs, and the programs that may ask for it.
    List,
    /// Puts a program on the list: its commands may ask for the Keychain, each time.
    Add {
        /// A program's name (gh), or git and a sub-command (git push).
        #[arg(required = true, num_args = 1..=2)]
        program: Vec<String>,
    },
    /// Takes a program off the list.
    Remove {
        /// A program's name (gh), or git and a sub-command (git push).
        #[arg(required = true, num_args = 1..=2)]
        program: Vec<String>,
    },
    /// Opens the Keychain to every command Glut runs, without asking, kept across sessions.
    Open,
    /// Shuts the Keychain again: only the listed programs may ask for it.
    Close,
}

/// Runs one `glut keychain` command.
pub fn run(command: KeychainCommand) -> ExitCode {
    let home = match crate::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    if !matches!(command, KeychainCommand::List)
        && std::env::var_os(crate::tools::shell::SANDBOXED_ENV).is_some()
    {
        return fail(say!(owners_only));
    }
    let keychain = Keychain::owner(&home);
    let outcome = match command {
        KeychainCommand::List => {
            list(&keychain);
            Ok(())
        }
        KeychainCommand::Add { program } => change(&keychain, &home, &program.join(" "), true),
        KeychainCommand::Remove { program } => change(&keychain, &home, &program.join(" "), false),
        KeychainCommand::Open => keychain
            .keep_open(&home, true)
            .map(|()| println!("{}", say!(keychain_opened))),
        KeychainCommand::Close => keychain
            .keep_open(&home, false)
            .map(|()| println!("{}", say!(keychain_closed))),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("glut keychain: {message}");
    ExitCode::from(1)
}

/// The state, then the programs one per line.
fn list(keychain: &Keychain) {
    // The owner's file: what `glut keychain open` keeps, wherever it counts.
    if keychain.kept_open() {
        println!("{}", say!(keychain_open));
    } else {
        println!("{}", say!(keychain_shut));
    }
    for program in keychain.programs() {
        println!("  {program}");
    }
}

fn change(
    keychain: &Keychain,
    home: &std::path::Path,
    program: &str,
    add: bool,
) -> Result<(), String> {
    let shown = crate::keychain::check_program(program)?;
    let changed = keychain.change(home, program, add)?;
    let text = match (add, changed) {
        (true, true) => say!(keychain_added, program = shown),
        (true, false) => say!(keychain_listed_already, program = shown),
        (false, true) => say!(keychain_removed, program = shown),
        (false, false) => say!(keychain_not_listed, program = shown),
    };
    println!("{text}");
    Ok(())
}
