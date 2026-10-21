//! `axh providers` (`docs/plans/glut-providers.md`, PV1, P6): the owner's providers in `~/.axh/providers.json`,
//! their keys in the secret store the owner chose. A key is read without echo, never taken as an argument.

use std::io::{BufRead as _, IsTerminal as _, Read as _, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use clap::Subcommand;
use glut::providers::catalog::{CATALOG, CUSTOM, SignIn};
use glut::providers::{self, Entry, KeyFrom, Providers};
use glut::{ChatModel, Message, OpenAiCompat};

/// The longest key `add` reads: far above any provider's, a stop for a file piped in by mistake.
const MAX_KEY_BYTES: u64 = 8 * 1024;
/// What `test` gives the model to answer.
const TEST_PROMPT: &str = "Answer with the single word OK.";
/// `test`'s output limit: room for a reasoning model's few thoughts before its word.
const TEST_MAX_TOKENS: u32 = 64;

#[derive(Subcommand)]
pub enum ProvidersCommand {
    /// The configured providers, the default, and where each one's key comes from (never the key).
    List,
    /// Every provider Glut knows by name — the kinds `add` takes.
    Catalog,
    /// Adds a provider, or replaces one of the same id. The key is asked for without echo — or read from stdin
    /// when it is no terminal (`op read … | axh providers add openrouter`).
    Add {
        /// A kind of `axh providers catalog`, or `openai-compatible` with --root.
        kind: String,
        /// The name runs use (`--provider`); the kind by default.
        #[arg(long)]
        id: Option<String>,
        /// The API's root, its version included (`http://box:8080/v1`); required for openai-compatible.
        #[arg(long)]
        root: Option<String>,
        /// The model a run takes when none is named.
        #[arg(long)]
        model: Option<String>,
        /// An environment variable the key may come from, over the catalog's.
        #[arg(long)]
        key_env: Option<String>,
        /// Store no key now: it comes from the environment.
        #[arg(long)]
        no_key: bool,
        /// An openai-compatible endpoint that needs no key (a server on this machine).
        #[arg(long)]
        keyless: bool,
        /// Make it the default provider (the first one added always is).
        #[arg(long)]
        default: bool,
    },
    /// Removes a provider and its stored key.
    Remove { id: String },
    /// Makes a provider the one runs take without --provider.
    Default { id: String },
    /// One tiny call to the provider's model, to see that endpoint, key and model work.
    Test {
        /// The provider; the default one when left out.
        id: Option<String>,
        /// Another model than the provider's default.
        #[arg(long)]
        model: Option<String>,
    },
}

/// Runs one `axh providers` command.
pub async fn run(command: ProvidersCommand) -> ExitCode {
    let home = match glut::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    let outcome = match command {
        ProvidersCommand::List => list(&home),
        ProvidersCommand::Catalog => {
            print_catalog();
            Ok(())
        }
        ProvidersCommand::Add {
            kind,
            id,
            root,
            model,
            key_env,
            no_key,
            keyless,
            default,
        } => add(
            &home,
            Added {
                kind,
                id,
                root,
                model,
                key_env,
                no_key,
                keyless,
                default,
            },
        ),
        ProvidersCommand::Remove { id } => remove(&home, &id),
        ProvidersCommand::Default { id } => make_default(&home, &id),
        ProvidersCommand::Test { id, model } => test(&home, id.as_deref(), model.as_deref()).await,
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("axh providers: {message}");
    ExitCode::from(1)
}

fn load(home: &Path) -> Result<Providers, String> {
    Providers::load(home).map_err(|err| err.to_string())
}

fn list(home: &Path) -> Result<(), String> {
    let providers = load(home)?;
    if providers.providers.is_empty() {
        println!("No provider yet: axh providers add <kind> (axh providers catalog lists them).");
        return Ok(());
    }
    let store = providers.store(home).map_err(|err| err.to_string())?;
    for (id, entry) in &providers.providers {
        let mark = if providers.default.as_deref() == Some(id.as_str()) {
            "*"
        } else {
            " "
        };
        // Resolved as a run would, so the line says which key a run takes; the model only fills the endpoint.
        let key =
            match providers::resolve(&providers, store.as_ref(), Some(id), Some("-"), |name| {
                std::env::var(name).ok()
            }) {
                Ok(resolved) => match resolved.key_from {
                    KeyFrom::Env(name) => format!("key from {name}"),
                    KeyFrom::Store => format!("key in {}", store.place()),
                    KeyFrom::NotNeeded => "no key needed".to_owned(),
                },
                Err(reason) => format!("NO KEY ({reason})"),
            };
        println!(
            "{mark} {id:<16} {:<20} {:<32} {}",
            entry.kind,
            entry.model.as_deref().unwrap_or("(no default model)"),
            key
        );
        println!("  {:<16} {}", "", entry.root());
    }
    Ok(())
}

fn print_catalog() {
    for known in CATALOG {
        let sign_in = match known.sign_in {
            SignIn::Key => known.key_env.unwrap_or("key"),
            SignIn::None => "no key",
        };
        println!(
            "{:<14} {:<28} {:<20} {}",
            known.id, known.name, sign_in, known.note
        );
    }
    println!(
        "{CUSTOM:<14} {:<28} {:<20} any other endpoint, with --root",
        "OpenAI-compatible", "key, or --keyless"
    );
}

/// `add`'s flags.
struct Added {
    kind: String,
    id: Option<String>,
    root: Option<String>,
    model: Option<String>,
    key_env: Option<String>,
    no_key: bool,
    keyless: bool,
    default: bool,
}

fn add(home: &Path, added: Added) -> Result<(), String> {
    let mut providers = load(home)?;
    let id = added.id.unwrap_or_else(|| added.kind.clone());
    providers::check_id(&id)?;
    let custom = added.kind == CUSTOM;
    if added.keyless && !custom {
        return Err("--keyless is only for openai-compatible; the catalog knows whether its providers need a key".into());
    }
    let entry = Entry {
        kind: added.kind,
        root: added.root,
        model: added.model,
        key_env: added.key_env,
        key: (custom && added.keyless).then_some(false),
    };
    // The same checks a load makes, before anything is asked or stored.
    entry.check()?;
    let store = providers.store(home).map_err(|err| err.to_string())?;
    let key = if entry.needs_key() && !added.no_key {
        Some(read_key(&id)?)
    } else {
        None
    };
    // A key stored for the entry's old root stays bound to that host: it is dropped, never carried to a new one.
    if let Some(old) = providers.providers.get(&id)
        && providers::account(&id, old) != providers::account(&id, &entry)
    {
        store
            .delete(&providers::account(&id, old))
            .map_err(|err| err.to_string())?;
    }
    let account = providers::account(&id, &entry);
    let first = providers.providers.is_empty();
    providers.providers.insert(id.clone(), entry.clone());
    if added.default || first || providers.default.is_none() {
        providers.default = Some(id.clone());
    }
    // The file first: a key stored for an entry that never got written would wait for nothing.
    providers.save(home).map_err(|err| err.to_string())?;
    match key {
        Some(key) => {
            store.set(&account, &key).map_err(|err| err.to_string())?;
            println!("The key of {id} is in {}.", store.place());
        }
        None if !entry.needs_key() => store.delete(&account).map_err(|err| err.to_string())?,
        None => {}
    }
    let default = if providers.default.as_deref() == Some(id.as_str()) {
        " (the default)"
    } else {
        ""
    };
    println!("Provider {id} added{default}. axh providers test {id} makes one tiny call.");
    Ok(())
}

/// The key, from the terminal without echo, or from stdin when it is no terminal.
fn read_key(id: &str) -> Result<String, String> {
    let stdin = std::io::stdin();
    let key = if stdin.is_terminal() {
        eprint!("Key for {id} (not shown): ");
        let _ = std::io::stderr().flush();
        let line = without_echo(|| {
            let mut line = String::new();
            stdin
                .lock()
                .take(MAX_KEY_BYTES + 1)
                .read_line(&mut line)
                .map(|_| line)
        });
        eprintln!();
        line.map_err(|err| format!("cannot read the key: {err}"))?
    } else {
        let mut text = String::new();
        stdin
            .lock()
            .take(MAX_KEY_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| format!("cannot read the key: {err}"))?;
        text
    };
    if key.len() as u64 > MAX_KEY_BYTES {
        return Err(format!(
            "more than {MAX_KEY_BYTES} bytes: no key is that long"
        ));
    }
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err("no key given (--no-key stores none, for a key from the environment)".into());
    }
    if key.chars().any(char::is_control) || key.contains(' ') {
        return Err(
            "the key holds spaces or control characters; was something else pasted?".into(),
        );
    }
    Ok(key)
}

/// Runs `read` with the terminal's echo off, and turns it back on whatever happens: on return, on a panic (the
/// guard) and on Ctrl-C (a signal task that restores it before the process ends).
fn without_echo<T>(read: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    use rustix::termios::{LocalModes, OptionalActions, Termios, tcgetattr, tcsetattr};

    /// Puts the terminal back as it was when dropped.
    struct Restore(Termios);

    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = tcsetattr(std::io::stdin(), OptionalActions::Flush, &self.0);
        }
    }

    let stdin = std::io::stdin();
    let before = tcgetattr(&stdin)?;
    let mut quiet = before.clone();
    quiet.local_modes.remove(LocalModes::ECHO);
    quiet.local_modes.insert(LocalModes::ECHONL);
    let on_signal = before.clone();
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = tcsetattr(std::io::stdin(), OptionalActions::Flush, &on_signal);
            eprintln!();
            std::process::exit(130);
        }
    });
    let restore = Restore(before);
    tcsetattr(&stdin, OptionalActions::Flush, &quiet)?;
    let result = read();
    drop(restore);
    interrupt.abort();
    result
}

fn remove(home: &Path, id: &str) -> Result<(), String> {
    let mut providers = load(home)?;
    let Some(removed) = providers.providers.remove(id) else {
        return Err(format!("no provider {id} (axh providers list shows them)"));
    };
    let store = providers.store(home).map_err(|err| err.to_string())?;
    if providers.default.as_deref() == Some(id) {
        providers.default = None;
    }
    providers.save(home).map_err(|err| err.to_string())?;
    store
        .delete(&providers::account(id, &removed))
        .map_err(|err| err.to_string())?;
    println!("Provider {id} removed, with its stored key.");
    match providers.providers.len() {
        0 => {}
        1 if providers.default.is_none() => {
            println!("Runs without --provider take the one left.");
        }
        _ if providers.default.is_none() => {
            println!("There is no default now: axh providers default <id>.");
        }
        _ => {}
    }
    Ok(())
}

fn make_default(home: &Path, id: &str) -> Result<(), String> {
    let mut providers = load(home)?;
    if !providers.providers.contains_key(id) {
        return Err(format!("no provider {id} (axh providers list shows them)"));
    }
    providers.default = Some(id.to_owned());
    providers.save(home).map_err(|err| err.to_string())?;
    println!("Runs without --provider now take {id}.");
    Ok(())
}

async fn test(home: &Path, id: Option<&str>, model: Option<&str>) -> Result<(), String> {
    let providers = load(home)?;
    let store = providers.store(home).map_err(|err| err.to_string())?;
    let resolved = providers::resolve(&providers, store.as_ref(), id, model, |name| {
        std::env::var(name).ok()
    })?;
    let name = format!("{} · {}", resolved.id, resolved.endpoint.model);
    let client = OpenAiCompat::new(resolved.endpoint).map_err(|err| err.to_string())?;
    let started = Instant::now();
    let reply = client
        .complete(&[Message::user(TEST_PROMPT)], &[], Some(TEST_MAX_TOKENS))
        .await
        .map_err(|err| format!("{name}: {err}"))?;
    let text: String = reply
        .message
        .content
        .as_deref()
        .unwrap_or("(no text)")
        .trim()
        .chars()
        .take(80)
        .collect();
    println!(
        "{name} answered in {:.1} s: {text} ({} tokens in, {} out)",
        started.elapsed().as_secs_f64(),
        reply.usage.input_tokens,
        reply.usage.output_tokens
    );
    Ok(())
}
