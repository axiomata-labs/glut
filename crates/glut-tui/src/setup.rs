//! The first start (TUI1c): without any provider Glut does not end with an error but asks for one — which kind, its
//! key (not shown as it is typed), which model — keeps it the way `glut providers add` does, and starts.

use std::io::{self, Write as _};
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use glut::providers::catalog::{self, CATALOG, SignIn};
use glut::providers::secrets::SecretStore;
use glut::providers::{self, Entry, Providers, models};

/// How many models the setup offers by number.
const OFFERED: usize = 15;

/// Asks for a first provider when there is none. `Ok(false)` when there was one already.
///
/// # Errors
///
/// A message when the owner gives up (an empty answer) or the provider cannot be kept.
pub async fn first_provider(home: &Path) -> Result<bool, String> {
    let mut list = Providers::load(home).map_err(|err| err.to_string())?;
    if !list.providers.is_empty() {
        return Ok(false);
    }
    println!("{}\n", tr!(setup_pick_provider));
    for (at, known) in CATALOG.iter().enumerate() {
        println!("  {:>2}  {:<26} {}", at + 1, known.name, known.note);
    }
    let answer = ask(&format!("\n{} ", tr!(setup_provider_prompt)))?;
    let known = answer
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|at| at.checked_sub(1))
        .and_then(|at| CATALOG.get(at))
        .or_else(|| catalog::known(answer.trim()))
        .ok_or(tr!(setup_no_provider))?;
    let mut entry = Entry {
        kind: known.id.to_owned(),
        root: None,
        model: None,
        key_env: None,
        key: None,
        windows: Default::default(),
    };
    let store = list.store(home).map_err(|err| err.to_string())?;
    let account = providers::account(known.id, &entry);
    if known.sign_in == SignIn::ChatGpt {
        glut::cli::providers::login(home, known.id, true).await?;
        list = Providers::load(home).map_err(|err| err.to_string())?;
    }
    if known.sign_in == SignIn::Key {
        // OpenRouter hands Glut a key at a sign-in in the browser (PV4b): an empty answer takes that way.
        let browser = providers::KEY_SIGN_INS.contains(&known.id);
        let asked = if browser {
            tr!(setup_key_or_sign_in, provider = known.name)
        } else {
            tr!(setup_key, provider = known.name)
        };
        let key = secret(&format!("{asked} "))?;
        if key.is_empty() && browser {
            glut::cli::providers::login(home, known.id, true).await?;
            list = Providers::load(home).map_err(|err| err.to_string())?;
        } else if key.is_empty() {
            return Err(tr!(setup_no_key).into());
        } else {
            store.set(&account, &key).map_err(|err| err.to_string())?;
            println!("{}", tr!(setup_key_kept, place = store.place()));
        }
    }
    list.providers.insert(known.id.to_owned(), entry.clone());
    entry.model = Some(choose_model(home, &list, store.as_ref(), known.id, known.name).await?);
    list.providers.insert(known.id.to_owned(), entry);
    list.default = Some(known.id.to_owned());
    list.save(home).map_err(|err| err.to_string())?;
    println!("{}\n", tr!(setup_provider_done, provider = known.name));
    Ok(true)
}

/// Asks for the model of a provider that has none — `glut providers add` without `--model` keeps it so — and keeps the
/// answer, so the start goes on instead of ending with "no model". Nothing to do when `--model` names one for this
/// run or the provider has its model.
///
/// # Errors
///
/// A message when the provider is unknown, gives no key, or the owner chooses no model.
pub async fn missing_model(
    home: &Path,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<(), String> {
    if model.is_some() {
        return Ok(());
    }
    let mut list = Providers::load(home).map_err(|err| err.to_string())?;
    let (id, entry) = list.pick(provider)?;
    if entry.model.is_some() {
        return Ok(());
    }
    let id = id.to_owned();
    let name = catalog::known(&entry.kind)
        .map_or(id.as_str(), |known| known.name)
        .to_owned();
    println!("{}\n", tr!(setup_pick_model, provider = name));
    let store = list.store(home).map_err(|err| err.to_string())?;
    let chosen = choose_model(home, &list, store.as_ref(), &id, &name).await?;
    if let Some(entry) = list.providers.get_mut(&id) {
        entry.model = Some(chosen);
    }
    list.save(home).map_err(|err| err.to_string())?;
    println!("{}\n", tr!(setup_model_done));
    Ok(())
}

/// The model the owner picks from the provider's list (the newest first), or types by id when there is no list.
async fn choose_model(
    home: &Path,
    list: &Providers,
    store: &dyn SecretStore,
    id: &str,
    name: &str,
) -> Result<String, String> {
    // `-` only stands in for the model to reach the endpoint: the list is what is asked for.
    let resolved = providers::resolve(list, store, Some(id), Some("-"), |name| {
        std::env::var(name).ok()
    })?;
    println!("{}", tr!(setup_asking_models, provider = name));
    let mut endpoint = resolved.endpoint.clone();
    if let Some(credential) = resolved.credential(home) {
        endpoint.api_key = Some(credential.bearer().await.map_err(|err| err.to_string())?);
    }
    let offered = match models::fetch(&endpoint, resolved.protocol).await {
        Ok(found) => {
            let _ = models::keep(home, id, &found);
            let mut usable: Vec<&models::ModelInfo> = found
                .models
                .iter()
                .filter(|model| found.allows(&model.id))
                .collect();
            usable.sort_by_key(|model| std::cmp::Reverse(model.created));
            usable
                .into_iter()
                .take(OFFERED)
                .map(|model| model.id.clone())
                .collect()
        }
        Err(reason) => {
            println!("{}", tr!(setup_no_list, reason = reason));
            Vec::new()
        }
    };
    for (at, id) in offered.iter().enumerate() {
        println!("  {:>2}  {}", at + 1, models::printable(id));
    }
    let answer = ask(&format!("{} ", tr!(setup_model_prompt)))?;
    answer
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|at| at.checked_sub(1))
        .and_then(|at| offered.get(at).cloned())
        .or_else(|| Some(answer.trim().to_owned()).filter(|id| !id.is_empty()))
        .ok_or_else(|| tr!(setup_no_model).to_owned())
}

/// A line typed by the owner.
fn ask(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    io::stdout().flush().map_err(|err| err.to_string())?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|err| err.to_string())?;
    Ok(line.trim().to_owned())
}

/// A secret typed by the owner: read key by key in raw mode, nothing of it shown, Ctrl+C gives up.
fn secret(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    io::stdout().flush().map_err(|err| err.to_string())?;
    terminal::enable_raw_mode().map_err(|err| err.to_string())?;
    let typed = (|| -> Result<String, String> {
        let mut secret = String::new();
        loop {
            match event::read().map_err(|err| err.to_string())? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Enter => return Ok(secret),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Err(tr!(setup_stopped).into());
                    }
                    KeyCode::Backspace => {
                        secret.pop();
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        secret.clear()
                    }
                    // A Ctrl or Alt chord is no character of the key.
                    KeyCode::Char(_)
                        if key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {}
                    KeyCode::Char(c) => secret.push(c),
                    _ => {}
                },
                _ => {}
            }
        }
    })();
    let _ = terminal::disable_raw_mode();
    println!();
    typed.map(|secret| secret.trim().to_owned())
}
