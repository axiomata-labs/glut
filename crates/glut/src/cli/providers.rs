//! `glut providers` (`docs/plans/glut-providers.md`, PV1, P6): the owner's providers in `~/.glut/providers.json`,
//! their keys in the secret store the owner chose. A key is read without echo, never taken as an argument.

use std::io::{BufRead as _, IsTerminal as _, Read as _, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use crate::providers::catalog::{CATALOG, CUSTOM, SignIn};
use crate::providers::{self, Entry, KeyFrom, Providers};
use crate::{ChatModel, Message, Model};
use clap::Subcommand;

/// The longest key `add` reads: far above any provider's, a stop for a file piped in by mistake.
const MAX_KEY_BYTES: u64 = 8 * 1024;
/// What `test` gives the model to answer.
const TEST_PROMPT: &str = "Answer with the single word OK.";
/// `test`'s output limit: room for a reasoning model's few thoughts before its word.
const TEST_MAX_TOKENS: u32 = 1024;

#[derive(Subcommand)]
pub enum ProvidersCommand {
    /// The configured providers, the default (marked *), and where each one's key comes from (never the key).
    List,
    /// Every provider Glut knows by name — the kinds `add` takes.
    Catalog,
    /// Adds a provider, or replaces one of the same id. The key is asked for without echo — or read from stdin
    /// when it is no terminal (`op read … | glut providers add openrouter`).
    Add {
        /// A kind of `glut providers catalog`, or `openai-compatible` with --root.
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

/// Runs one `glut providers` command.
pub async fn run(command: ProvidersCommand) -> ExitCode {
    let home = match crate::permissions::settings::home_dir() {
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
    eprintln!("glut providers: {message}");
    ExitCode::from(1)
}

fn load(home: &Path) -> Result<Providers, String> {
    Providers::load(home).map_err(|err| err.to_string())
}

fn list(home: &Path) -> Result<(), String> {
    let providers = load(home)?;
    if providers.providers.is_empty() {
        println!("{}", say!(providers_none));
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
                    KeyFrom::Env(name) => say!(key_from_env, name = name),
                    KeyFrom::Store => say!(key_in_store, place = store.place()),
                    KeyFrom::NotNeeded => say!(key_not_needed).to_owned(),
                },
                Err(reason) => say!(key_missing, reason = reason),
            };
        println!(
            "{mark} {id:<16} {:<20} {:<32} {}",
            entry.kind,
            entry.model.as_deref().unwrap_or(say!(no_default_model)),
            key
        );
        println!("  {:<16} {}", "", entry.root());
    }
    Ok(())
}

fn print_catalog() {
    for known in CATALOG {
        let sign_in = match known.sign_in {
            SignIn::Key => known.key_env.unwrap_or(say!(catalog_key)),
            SignIn::None => say!(catalog_no_key),
        };
        println!(
            "{:<14} {:<28} {:<20} {}",
            known.id, known.name, sign_in, known.note
        );
    }
    println!(
        "{CUSTOM:<14} {:<28} {:<20} {}",
        "OpenAI-compatible",
        say!(catalog_custom_sign_in),
        say!(catalog_custom_note)
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
        return Err(say!(keyless_only_custom).into());
    }
    let entry = Entry {
        kind: added.kind,
        root: added.root,
        model: added.model,
        key_env: added.key_env,
        key: (custom && added.keyless).then_some(false),
        // A provider added again keeps the windows the owner set for its models.
        windows: providers
            .providers
            .get(&id)
            .map(|old| old.windows.clone())
            .unwrap_or_default(),
    };
    // The same checks a load makes, before anything is asked or stored.
    entry.check()?;
    let store = providers.store(home).map_err(|err| err.to_string())?;
    // A provider whose account's key another one keeps (OpenCode Zen's for OpenCode Go) may take that one: Enter.
    let shared = providers::shared_key(&providers, store.as_ref(), &id, &entry)?.is_some();
    let key = if entry.needs_key() && !added.no_key {
        read_key(&id, shared)?
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
            println!("{}", say!(key_kept, id = id, place = store.place()));
        }
        // A key of its own, given before, stays: Enter changes nothing stored.
        None if shared => println!("{}", say!(key_shared, id = id)),
        None if !entry.needs_key() => store.delete(&account).map_err(|err| err.to_string())?,
        None => {}
    }
    let default = if providers.default.as_deref() == Some(id.as_str()) {
        say!(the_default)
    } else {
        ""
    };
    println!("{}", say!(provider_added, id = id, default = default));
    Ok(())
}

/// The key, from the terminal without echo, or from stdin when it is no terminal. `None` when `shared` (another
/// provider of the account keeps one) and nothing was typed.
fn read_key(id: &str, shared: bool) -> Result<Option<String>, String> {
    let stdin = std::io::stdin();
    let key = if stdin.is_terminal() {
        if shared {
            eprint!("{}", say!(key_prompt_shared, id = id));
        } else {
            eprint!("{}", say!(key_prompt, id = id));
        }
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
        line.map_err(|err| say!(key_unreadable, err = err))?
    } else {
        let mut text = String::new();
        stdin
            .lock()
            .take(MAX_KEY_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| say!(key_unreadable, err = err))?;
        text
    };
    if key.len() as u64 > MAX_KEY_BYTES {
        return Err(say!(key_too_long, max = MAX_KEY_BYTES));
    }
    let key = key.trim().to_owned();
    if key.is_empty() && shared {
        return Ok(None);
    }
    if key.is_empty() {
        return Err(say!(no_key_given).into());
    }
    if key.chars().any(char::is_control) || key.contains(' ') {
        return Err(say!(key_odd).into());
    }
    Ok(Some(key))
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
        return Err(say!(no_such_provider, id = id));
    };
    let store = providers.store(home).map_err(|err| err.to_string())?;
    if providers.default.as_deref() == Some(id) {
        providers.default = None;
    }
    providers.save(home).map_err(|err| err.to_string())?;
    store
        .delete(&providers::account(id, &removed))
        .map_err(|err| err.to_string())?;
    println!("{}", say!(provider_removed, id = id));
    match providers.providers.len() {
        0 => {}
        1 if providers.default.is_none() => {
            println!("{}", say!(one_left));
        }
        _ if providers.default.is_none() => {
            println!("{}", say!(no_default_now));
        }
        _ => {}
    }
    Ok(())
}

fn make_default(home: &Path, id: &str) -> Result<(), String> {
    let mut providers = load(home)?;
    if !providers.providers.contains_key(id) {
        return Err(say!(no_such_provider, id = id));
    }
    providers.default = Some(id.to_owned());
    providers.save(home).map_err(|err| err.to_string())?;
    println!("{}", say!(default_set, id = id));
    Ok(())
}

async fn test(home: &Path, id: Option<&str>, model: Option<&str>) -> Result<(), String> {
    let providers = load(home)?;
    let store = providers.store(home).map_err(|err| err.to_string())?;
    let resolved = providers::resolve(&providers, store.as_ref(), id, model, |name| {
        std::env::var(name).ok()
    })?;
    let name = format!("{} · {}", resolved.id, resolved.endpoint.model);
    // A gateway's model needs its list to be routed; without one its API is guessed from its id.
    let list = if resolved.protocol == crate::providers::catalog::Protocol::Gateway {
        providers::models::current(home, &resolved.id, &resolved.endpoint, resolved.protocol)
            .await
            .ok()
    } else {
        None
    };
    let (endpoint, protocol) =
        providers::route(resolved.endpoint, resolved.protocol, list.as_ref())?;
    let client = Model::new(endpoint, protocol).map_err(|err| err.to_string())?;
    let started = Instant::now();
    let reply = client
        .complete(&[Message::user(TEST_PROMPT)], &[], Some(TEST_MAX_TOKENS))
        .await
        .map_err(|err| format!("{name}: {err}"))?;
    let text: String = reply
        .message
        .content
        .as_deref()
        .unwrap_or(say!(test_no_text))
        .trim()
        .chars()
        .take(80)
        .collect();
    println!(
        "{}",
        say!(
            test_answered,
            name = name,
            secs = format!("{:.1}", started.elapsed().as_secs_f64()),
            text = text,
            input = reply.usage.input_tokens,
            output = reply.usage.output_tokens
        )
    );
    Ok(())
}
