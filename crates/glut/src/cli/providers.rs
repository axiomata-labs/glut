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
const MAX_KEY_BYTES: u64 = providers::MAX_KEY_BYTES;
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
    /// Changes a provider's key alone — its model, keyEnv and root stay as they are. The key is read without echo and
    /// checked with one small call before the old one is replaced.
    Key {
        /// The provider whose key is changed.
        id: String,
    },
    /// Signs in in the browser — your ChatGPT Plus or Pro plan (glut providers login chatgpt), or OpenRouter for a
    /// key without copying it (glut providers login openrouter).
    Login {
        /// The provider; one of the catalog's that sign in is added on the way.
        id: String,
    },
    /// Signs out of a subscription: the sign-in revoked at its provider and forgotten here.
    Logout {
        /// The provider.
        id: String,
    },
    /// While Glut is developed: moves every key to the file ~/.glut/credentials.json, so a new build asks the
    /// Keychain nothing, or back to the Keychain.
    #[cfg(all(feature = "dev-keys", target_os = "macos"))]
    Store {
        /// file or keychain.
        place: StorePlace,
    },
    /// One tiny call to the provider's model, to see that endpoint, key and model work.
    Test {
        /// The provider; the default one when left out.
        id: Option<String>,
        /// Another model than the provider's default.
        #[arg(long)]
        model: Option<String>,
    },
}

/// Where `glut providers store` moves the keys (T87): `~/.glut/credentials.json` or the login Keychain.
#[cfg(all(feature = "dev-keys", target_os = "macos"))]
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum StorePlace {
    // Named without doc comments: clap would turn them into a long help of its own, untranslated.
    File,
    Keychain,
}

/// Runs one `glut providers` command.
pub async fn run(command: ProvidersCommand) -> ExitCode {
    let home = match crate::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    let writes = !matches!(
        command,
        ProvidersCommand::List | ProvidersCommand::Catalog | ProvidersCommand::Test { .. }
    );
    if writes && std::env::var_os(crate::tools::shell::SANDBOXED_ENV).is_some() {
        return fail(say!(owners_only));
    }
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
        ProvidersCommand::Key { id } => new_key(&home, &id).await,
        ProvidersCommand::Test { id, model } => test(&home, id.as_deref(), model.as_deref()).await,
        ProvidersCommand::Login { id } => login(&home, &id, false).await,
        ProvidersCommand::Logout { id } => logout(&home, &id).await,
        #[cfg(all(feature = "dev-keys", target_os = "macos"))]
        ProvidersCommand::Store { place } => move_store(&home, place),
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
    if crate::providers::secrets::file_ignored(providers.credential_store) {
        println!("{}", say!(store_file_ignored));
    }
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
                    KeyFrom::SignedIn => say!(key_signed_in).to_owned(),
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

/// `glut providers store` (T87): every key copied to the place named, the choice saved, then — back in the Keychain
/// — the file emptied, again on every later call while it holds any; the Keychain's own keys stay where they are when
/// the file takes over (Q3).
#[cfg(all(feature = "dev-keys", target_os = "macos"))]
fn move_store(home: &Path, place: StorePlace) -> Result<(), String> {
    use crate::providers::secrets::{self, StoreChoice};
    owners_home(home)?;
    let mut providers = load(home)?;
    let open = |choice| secrets::open(choice, home).map_err(|err| err.to_string());
    let (file, keychain) = (open(StoreChoice::File)?, open(StoreChoice::Keyring)?);
    let to_file = matches!(place, StorePlace::File);
    let already = (providers.credential_store == StoreChoice::File) == to_file;
    let (from, to) = if to_file {
        (&keychain, &file)
    } else {
        (&file, &keychain)
    };
    let moved = if already {
        println!("{}", say!(store_already, place = to.place()));
        None
    } else {
        let copied =
            secrets::copy_secrets(from.as_ref(), to.as_ref()).map_err(|err| err.to_string())?;
        providers.credential_store = if to_file {
            StoreChoice::File
        } else {
            StoreChoice::Auto
        };
        providers.save(home).map_err(|err| err.to_string())?;
        Some(copied)
    };
    if !to_file {
        // Also where the keys were already back: a cleanup that failed before is done now.
        secrets::forget_all(file.as_ref()).map_err(|err| say!(store_file_left, reason = err))?;
    }
    if let Some(count) = moved {
        println!("{}", say!(store_moved, count = count, place = to.place()));
        if to_file {
            println!("{}", say!(store_file_note));
        }
    }
    Ok(())
}

/// Refuses a home other than the owner's own (T87, security audit): `GLUT_HOME` or `HOME` are a command's to set, and
/// a sandboxed command pointing them at a folder it may read would get every key copied there — the sandbox only
/// keeps it from the real `~/.glut`. The account's home comes from the system's user database, never the environment.
#[cfg(all(feature = "dev-keys", target_os = "macos"))]
fn owners_home(home: &Path) -> Result<(), String> {
    // `id -P`: the current user's passwd line, its ninth field the home folder.
    let out = std::process::Command::new("/usr/bin/id")
        .arg("-P")
        .output()
        .map_err(|err| err.to_string())?;
    let line = String::from_utf8_lossy(&out.stdout);
    let owners = line
        .trim_end()
        .split(':')
        .nth(8)
        .filter(|dir| dir.starts_with('/'));
    let real = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match owners {
        Some(dir) if real(Path::new(dir)) == real(home) => Ok(()),
        _ => Err(say!(owners_only).to_owned()),
    }
}

fn print_catalog() {
    for known in CATALOG {
        let sign_in = match known.sign_in {
            SignIn::Key => known.key_env.unwrap_or(say!(catalog_key)),
            SignIn::None => say!(catalog_no_key),
            SignIn::ChatGpt => say!(catalog_sign_in),
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
        // And the hosts the owner chose for them (T90).
        routing: providers
            .providers
            .get(&id)
            .map(|old| old.routing.clone())
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
    let prompt = if shared {
        say!(key_prompt_shared, id = id)
    } else {
        say!(key_prompt, id = id)
    };
    read_key_from(prompt, shared)
}

/// The new key of `glut providers key <id>`, read exactly as `add` reads one (T70) — asked as what it is.
fn read_new_key(id: &str) -> Result<String, String> {
    read_key_from(say!(key_prompt_new, id = id), false)?
        .ok_or_else(|| say!(no_key_given).to_owned())
}

/// [`read_key`]'s body: `prompt` on stderr, then the line itself.
fn read_key_from(prompt: String, shared: bool) -> Result<Option<String>, String> {
    let stdin = std::io::stdin();
    let key = if stdin.is_terminal() {
        eprint!("{prompt}");
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
pub(super) fn without_echo<T>(read: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
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

/// `glut providers key <id>` (T70): the new key read without echo, checked against the provider's model list before
/// the old one is replaced, and kept — or the old one stays where it is, as the owner says. Nothing else of the entry
/// is touched: its model, its `keyEnv` and its root stay as they are.
async fn new_key(home: &Path, id: &str) -> Result<(), String> {
    let providers = load(home)?;
    let store = providers.store(home).map_err(|err| err.to_string())?;
    let (id, account) =
        match providers::key_change(&providers, Some(id), |name| std::env::var(name).ok())? {
            providers::KeyChange::Asked { id, account } => (id, account),
            providers::KeyChange::SignedIn(id) => return Err(say!(key_sign_in, id = id)),
            providers::KeyChange::NotNeeded(id) => return Err(say!(key_no_key_needed, id = id)),
            providers::KeyChange::Env { id, name } => {
                return Err(say!(key_env_now, id = id, name = name));
            }
        };
    // Typed by the owner, never piped: a key changed by a script would be no one's word.
    if !std::io::stdin().is_terminal() {
        return Err(say!(key_needs_terminal).into());
    }
    let key = read_new_key(&account)?;
    eprintln!("{}", say!(key_checking, id = id));
    let kept = providers::change_key(
        store.as_ref(),
        &account,
        &key,
        providers::proves(&providers, &id, &key),
        |reason| {
            eprintln!(
                "{}",
                say!(
                    key_check_failed,
                    reason = crate::permissions::printable(reason)
                )
            );
            async { keep_anyway() }
        },
    )
    .await?;
    match kept {
        providers::Kept::New => println!("{}", say!(key_changed, id = id, place = store.place())),
        providers::Kept::Old => println!("{}", say!(key_kept_old, id = id)),
    }
    Ok(())
}

/// The owner's word when the new key does not check out (T70): `y` or `Y` keeps it anyway, everything else — an empty
/// line, a piped stdin — keeps the old key.
fn keep_anyway() -> bool {
    eprint!("{}", say!(key_keep_anyway));
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    eprintln!();
    read.is_ok() && matches!(line.trim().chars().next(), Some('y' | 'Y'))
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
    let credential = resolved.credential(home);
    let (endpoint, protocol) =
        providers::route(resolved.endpoint, resolved.protocol, list.as_ref())?;
    let client = Model::signed(endpoint, protocol, credential).map_err(|err| err.to_string())?;
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
            secs = crate::lang::decimal(started.elapsed().as_secs_f64(), 1),
            text = text,
            input = reply.usage.input_tokens,
            output = reply.usage.output_tokens
        )
    );
    Ok(())
}

/// `glut providers login <id>`: the browser's sign-in, opened on the owner's Enter, the tokens — or OpenRouter's key —
/// kept in the store; also Glut's first start in the terminal, where the owner picked a sign-in.
///
/// # Errors
///
/// Why there is no sign-in, for the owner to read.
pub async fn login(home: &Path, id: &str, setup: bool) -> Result<(), String> {
    use crate::subscription::chatgpt;
    if providers::gives_key_by_sign_in(home, id) {
        return login_for_key(home, id, setup).await;
    }
    let account = providers::signing(home, id)?;
    let kept = crate::subscription::load(home, &account).map_err(|err| err.to_string())?;
    let sign_in = chatgpt::begin(home, kept, chatgpt::Server::openai()).await?;
    let heading = say!(plan_sign_in_at, host = sign_in.host());
    let pasted = await_browser(heading, sign_in.address(), setup).await?;
    let record = sign_in
        .finish(pasted)
        .await
        .map_err(|err| err.to_string())?;
    // Kept under the record's lock: a renewal of another window must not write its older tokens over these.
    let _lock = crate::subscription::Lock::take(home, &account)
        .await
        .map_err(|err| err.to_string())?;
    crate::subscription::save(home, &account, &record).map_err(|err| err.to_string())?;
    let who = record
        .email
        .clone()
        .unwrap_or_else(|| record.subject.clone());
    println!(
        "{}",
        say!(
            plan_signed_in,
            id = id,
            who = crate::permissions::printable(&who)
        )
    );
    Ok(())
}

/// The sign-in's address said, and the browser awaited: an empty line opens it (the owner's word), any other is the
/// address pasted back (over SSH). As `glut mcp login` reads it, on a thread of its own; the first start reads one
/// line and no more: its next question is the terminal's, and a thread still reading would take the owner's answer.
async fn await_browser(
    heading: String,
    address: &str,
    setup: bool,
) -> Result<impl std::future::Future<Output = Option<String>> + use<>, String> {
    println!("{heading}");
    println!("{address}");
    let local = crate::oauth::can_open();
    println!(
        "{}",
        if local {
            say!(mcp_enter_opens)
        } else {
            say!(mcp_open_yourself)
        }
    );
    let (send, received) = tokio::sync::oneshot::channel::<String>();
    if setup {
        let line = tokio::task::spawn_blocking(|| {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).map(|_| line)
        })
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
        let line = line.trim().to_owned();
        if !line.is_empty() {
            let _ = send.send(line.chars().take(MAX_KEY_BYTES as usize).collect());
        } else if local && crate::oauth::open_in_browser(address) {
            println!("{}", say!(mcp_browser_opened));
        }
    } else if std::io::stdin().is_terminal() {
        let address = address.to_owned();
        std::thread::spawn(move || {
            let mut lines = std::io::stdin().lock().lines();
            while let Some(Ok(line)) = lines.next() {
                let line = line.trim();
                if !line.is_empty() {
                    let _ = send.send(line.chars().take(MAX_KEY_BYTES as usize).collect());
                    return;
                }
                if local && crate::oauth::open_in_browser(&address) {
                    println!("{}", say!(mcp_browser_opened));
                }
            }
        });
    }
    Ok(async { received.await.ok() })
}

/// `glut providers login openrouter` (PV4b): signed in in the browser, the key OpenRouter hands out kept like one
/// typed — never shown.
async fn login_for_key(home: &Path, id: &str, setup: bool) -> Result<(), String> {
    use crate::providers::openrouter;
    providers::check_key_sign_in(home, id)?;
    let sign_in = openrouter::begin(openrouter::Server::openrouter()).await?;
    let heading = say!(key_sign_in_at, host = sign_in.host());
    let pasted = await_browser(heading, sign_in.address(), setup).await?;
    let key = sign_in.finish(pasted).await?;
    let (place, replaced) = providers::keep_signed_key(home, id, &key)?;
    println!("{}", say!(key_kept, id = id, place = place));
    if replaced {
        println!("{}", say!(key_replaced, id = id));
    }
    Ok(())
}

/// `glut providers logout <id>`: the sign-in revoked at the provider, the tokens gone; the client stays for the
/// next sign-in.
async fn logout(home: &Path, id: &str) -> Result<(), String> {
    use crate::subscription::chatgpt::{self, SignedOut};
    let providers = load(home)?;
    let entry = providers
        .providers
        .get(id)
        .ok_or_else(|| say!(no_such_provider, id = id))?;
    if !entry.signs_in() {
        return Err(say!(not_a_sign_in, id = id));
    }
    let account = providers::account(id, entry);
    // Under the record's lock: a renewal racing the sign-out would keep the account signed in.
    let _lock = crate::subscription::Lock::take(home, &account)
        .await
        .map_err(|err| err.to_string())?;
    let Some(record) = crate::subscription::load(home, &account).map_err(|err| err.to_string())?
    else {
        println!("{}", say!(plan_out_nothing, id = id));
        return Ok(());
    };
    let (kept, done) = chatgpt::sign_out(&chatgpt::Server::openai(), &record).await;
    crate::subscription::save(home, &account, &kept).map_err(|err| err.to_string())?;
    println!(
        "{}",
        match done {
            SignedOut::Nothing => say!(plan_out_nothing, id = id),
            SignedOut::Revoked => say!(plan_out_revoked, id = id),
            SignedOut::NotConfirmed(reason) => say!(
                plan_out_unconfirmed,
                id = id,
                reason = crate::permissions::printable(&reason)
            ),
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry as one of the tests' providers files holds it.
    fn entry(kind: &str) -> Entry {
        Entry {
            kind: kind.into(),
            root: None,
            model: Some("deepseek/deepseek-v4.1-flash".into()),
            key_env: None,
            key: None,
            windows: std::collections::BTreeMap::new(),
            routing: Default::default(),
        }
    }

    /// T70 (Q3): `glut providers key` says why there is none to change — a sign-in, a provider that needs no key —
    /// and reads no key at all before that. The third refusal (a key from the environment) is the library's, where
    /// the variable can be given to it (`providers::tests::a_provider_with_no_key_to_change_is_told_so…`).
    #[tokio::test]
    async fn key_says_why_there_is_no_key_to_change_before_it_reads_one() {
        let home = std::env::temp_dir().join(format!(
            "glut-provider-key-cli-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        let mut server = entry(CUSTOM);
        server.root = Some("http://localhost:8080/v1".into());
        server.key = Some(false);
        let mut list = Providers::default();
        list.providers.insert("chatgpt".into(), entry("chatgpt"));
        list.providers.insert("box".into(), server);
        list.providers.insert("local".into(), entry("ollama"));
        list.save(&home).unwrap();
        assert_eq!(
            new_key(&home, "chatgpt").await.unwrap_err(),
            say!(key_sign_in, id = "chatgpt")
        );
        assert_eq!(
            new_key(&home, "box").await.unwrap_err(),
            say!(key_no_key_needed, id = "box")
        );
        assert_eq!(
            new_key(&home, "local").await.unwrap_err(),
            say!(key_no_key_needed, id = "local")
        );
        let missing = new_key(&home, "nonesuch").await.unwrap_err();
        assert!(missing.contains("no provider nonesuch"), "{missing}");
        let _ = std::fs::remove_dir_all(&home);
    }
}
