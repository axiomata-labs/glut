//! Glut's own providers (`docs/plans/glut-providers.md`, PV1): the catalog of known ones, the owner's configured
//! ones in `~/.axh/providers.json`, and their keys in a secret store. A run named by `--provider` (or the default
//! provider) gets its endpoint from here; a host that passes its own endpoint never comes here (P7).

pub mod catalog;
pub mod secrets;

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ModelEndpoint;
use crate::error::HarnessError;
use catalog::{CUSTOM, SignIn};
use secrets::{SecretStore, StoreChoice};

/// The owner's folder below the home folder (as `permissions::settings::USER_DIR`).
pub const DIR: &str = ".axh";
/// The configured providers, below [`DIR`]. Glut writes it (`axh providers add`), the owner may edit it; it never
/// holds a secret. A file of its own rather than a part of `settings.json`: Glut rewrites it whole, and the owner's
/// hand-written settings are never rewritten by a command.
pub const FILE: &str = "providers.json";
/// A providers file larger than this is none.
const MAX_FILE_BYTES: u64 = 256 * 1024;
/// The longest provider id.
const MAX_ID_CHARS: usize = 40;

/// `~/.axh/providers.json`. Unknown fields are refused: a field a later Glut writes (a sign-in kind) must not be
/// silently dropped by an older one that would then treat the provider as something else.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Providers {
    /// The provider a run without `--provider` takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Where keys are kept (P4).
    #[serde(default)]
    pub credential_store: StoreChoice,
    #[serde(default)]
    pub providers: BTreeMap<String, Entry>,
}

/// One configured provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    /// A catalog id, or `openai-compatible` for an endpoint named by its root.
    pub kind: String,
    /// The API's root, its version included (`http://box:8080/v1`): required for `openai-compatible`, else it
    /// overrides the catalog's (a proxy, a region).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// The model a run takes when none is named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// An environment variable the key may come from, over the catalog's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    /// Whether a run needs a key: `false` for a local server. Only for `openai-compatible`; the catalog knows its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<bool>,
}

impl Providers {
    /// The file below `home`.
    pub fn path(home: &Path) -> PathBuf {
        home.join(DIR).join(FILE)
    }

    /// Reads `~/.axh/providers.json`; a missing file has no providers.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] for a file that cannot be read, is too large, or is not of this shape.
    pub fn load(home: &Path) -> Result<Self, HarnessError> {
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        let file = match open_no_follow(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(fail(err.to_string())),
        };
        if !file
            .metadata()
            .map_err(|err| fail(err.to_string()))?
            .is_file()
        {
            return Err(fail("not a regular file".into()));
        }
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| fail(err.to_string()))?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(fail(format!("larger than {MAX_FILE_BYTES} bytes")));
        }
        let providers: Self = serde_json::from_str(&text).map_err(|err| fail(err.to_string()))?;
        for (id, entry) in &providers.providers {
            check_id(id).map_err(fail)?;
            entry
                .check()
                .map_err(|reason| fail(format!("{id}: {reason}")))?;
        }
        Ok(providers)
    }

    /// Writes the file whole, through a new file renamed into place.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when it cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), HarnessError> {
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        let dir = home.join(DIR);
        make_private_dir(&dir).map_err(|err| fail(err.to_string()))?;
        let mut text = serde_json::to_string_pretty(self).map_err(|err| fail(err.to_string()))?;
        text.push('\n');
        write_replacing(&dir, &path, text.as_bytes(), 0o644).map_err(|err| fail(err.to_string()))
    }

    /// The secret store the owner chose.
    ///
    /// # Errors
    ///
    /// See [`secrets::open`].
    pub fn store(&self, home: &Path) -> Result<Box<dyn SecretStore>, HarnessError> {
        secrets::open(self.credential_store, home)
    }

    /// The provider `id`, or the default one, or the only one.
    ///
    /// # Errors
    ///
    /// A message when there is no such provider, or no default and no id.
    pub fn pick(&self, id: Option<&str>) -> Result<(&str, &Entry), String> {
        let id = match id.or(self.default.as_deref()) {
            Some(id) => id,
            None if self.providers.len() == 1 => self
                .providers
                .keys()
                .next()
                .map(String::as_str)
                .unwrap_or(""),
            None => {
                return Err(if self.providers.is_empty() {
                    "no provider yet: axh providers add <kind> (axh providers catalog lists them)"
                        .into()
                } else {
                    "no default provider: name one with --provider, or set it with axh providers default <id>".into()
                });
            }
        };
        self.providers
            .get_key_value(id)
            .map(|(id, entry)| (id.as_str(), entry))
            .ok_or_else(|| format!("no provider {id} (axh providers list shows them)"))
    }
}

/// Opens `path` for reading without following a link in its last component.
pub(crate) fn open_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)
}

/// `~/.axh` as only the owner may list it — the folder of the keys file and the providers.
pub(crate) fn make_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Writes `bytes` to `path` whole: into a new file of `mode` beside it (never an existing one, never through a
/// link), synced, then renamed into place.
pub(crate) fn write_replacing(
    dir: &Path,
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// A provider id: lowercase letters, digits and dashes.
///
/// # Errors
///
/// A message naming what is wrong.
pub fn check_id(id: &str) -> Result<(), String> {
    let fine = !id.is_empty()
        && id.chars().count() <= MAX_ID_CHARS
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-');
    if fine {
        Ok(())
    } else {
        Err(format!(
            "provider id {id:?}: lowercase letters, digits and dashes, at most {MAX_ID_CHARS}"
        ))
    }
}

impl Entry {
    /// What a load requires of an entry.
    ///
    /// # Errors
    ///
    /// A message naming what is wrong.
    pub fn check(&self) -> Result<(), String> {
        if self.kind == CUSTOM {
            if self.root.is_none() {
                return Err(
                    "an openai-compatible provider needs its root (\"root\": \"http://…/v1\")"
                        .into(),
                );
            }
        } else if catalog::known(&self.kind).is_none() {
            return Err(format!(
                "unknown kind {:?} (axh providers catalog lists them)",
                self.kind
            ));
        } else if self.key.is_some() {
            return Err("\"key\" is only for openai-compatible; the catalog knows whether its providers need one".into());
        }
        if let Some(root) = &self.root {
            check_root(root)?;
        }
        if self.needs_key() && !crate::model::secure_enough(self.root()) {
            return Err(format!(
                "a key is only sent over https, or over http to this machine — not to {}",
                self.root()
            ));
        }
        if let Some(name) = &self.key_env
            && (name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        {
            return Err(format!("keyEnv {name:?} is no environment variable's name"));
        }
        Ok(())
    }

    /// The API's root.
    pub fn root(&self) -> &str {
        self.root
            .as_deref()
            .or_else(|| catalog::known(&self.kind).map(|known| known.root))
            .unwrap_or_default()
    }

    /// Whether a run needs a key.
    pub fn needs_key(&self) -> bool {
        match catalog::known(&self.kind) {
            Some(known) => known.sign_in == SignIn::Key,
            None => self.key.unwrap_or(true),
        }
    }

    /// The environment variable the key may come from.
    pub fn key_env(&self) -> Option<&str> {
        self.key_env
            .as_deref()
            .or_else(|| catalog::known(&self.kind).and_then(|known| known.key_env))
    }
}

/// An API root: http(s), no user or password in it, no query or fragment.
///
/// # Errors
///
/// A message naming what is wrong.
pub fn check_root(root: &str) -> Result<(), String> {
    let Some(rest) = root
        .strip_prefix("https://")
        .or_else(|| root.strip_prefix("http://"))
    else {
        return Err(format!("root {root:?}: http:// or https://"));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(format!("root {root:?} names no host"));
    }
    if authority.contains('@') {
        return Err(format!(
            "root {root:?} holds a user or a password; the key goes in the secret store"
        ));
    }
    if root.contains(['?', '#']) {
        return Err(format!("root {root:?}: no query or fragment"));
    }
    Ok(())
}

/// Every environment variable the owner's providers name for their keys (`keyEnv`), read from the owner's
/// `providers.json` — never an MCP server's to have, like the catalog's (`mcp::client`). An unreadable file names none.
pub fn configured_key_envs() -> Vec<String> {
    crate::permissions::settings::home_dir()
        .ok()
        .and_then(|home| Providers::load(&home).ok())
        .map(|providers| {
            providers
                .providers
                .into_values()
                .filter_map(|entry| entry.key_env)
                .collect()
        })
        .unwrap_or_default()
}

/// Where a run's key came from — said by `axh providers list`, never the key itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyFrom {
    /// The provider's environment variable.
    Env(String),
    /// The secret store.
    Store,
    /// The provider needs none.
    NotNeeded,
}

/// A provider made into a run's endpoint.
#[derive(Debug)]
pub struct Resolved {
    pub id: String,
    pub endpoint: ModelEndpoint,
    pub key_from: KeyFrom,
}

/// The endpoint of provider `id` (or the default one) for `model` (or its default model): the key from the
/// provider's environment variable, else the secret store. `AXH_API_KEY` is not read here — it belongs to a one-off
/// `--base-url`, and taken for every provider it would go to whichever root `--provider` names (security review of
/// PV1).
///
/// # Errors
///
/// A message when the provider is unknown, has no model, or needs a key that is nowhere.
pub fn resolve(
    providers: &Providers,
    store: &dyn SecretStore,
    id: Option<&str>,
    model: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Resolved, String> {
    let (id, entry) = providers.pick(id)?;
    let model = model.or(entry.model.as_deref()).ok_or_else(|| {
        format!("no model for {id}: name one with --model, or set one with axh providers add")
    })?;
    // A key pasted into a variable or a store keeps no newline: it would go into the header with it.
    let trimmed = |value: String| Some(value.trim().to_owned()).filter(|value| !value.is_empty());
    let (api_key, key_from) = if !entry.needs_key() {
        (None, KeyFrom::NotNeeded)
    } else if let Some((name, key)) = entry
        .key_env()
        .and_then(|name| env(name).and_then(trimmed).map(|key| (name, key)))
    {
        (Some(key), KeyFrom::Env(name.to_owned()))
    } else {
        let stored = store
            .get(&account(id, entry))
            .map_err(|err| err.to_string())?
            .and_then(trimmed);
        match stored {
            Some(key) => (Some(key), KeyFrom::Store),
            None => {
                let root = if entry.kind == CUSTOM {
                    format!(" --root {}", entry.root())
                } else {
                    String::new()
                };
                return Err(format!(
                    "no key for {id} at {}: axh providers add {}{root} --id {id} puts one in {}",
                    host(entry.root()),
                    entry.kind,
                    store.place()
                ));
            }
        }
    };
    Ok(Resolved {
        id: id.to_owned(),
        endpoint: ModelEndpoint {
            base_url: entry.root().trim_end_matches('/').to_owned(),
            api_key,
            model: model.to_owned(),
            effort: None,
            versioned: true,
        },
        key_from,
    })
}

/// The account a provider's key is stored under: its id and the host of its root (`openrouter@openrouter.ai`). A
/// key is bound to where it was given for — a root changed in `providers.json` afterwards finds no key, instead of
/// sending the stored one to the new host (security review of PV1).
pub fn account(id: &str, entry: &Entry) -> String {
    format!("{id}@{}", host(entry.root()))
}

/// The host (with its port) of an API root.
fn host(root: &str) -> &str {
    let rest = root.split_once("://").map_or(root, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A store in memory.
    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<String, String>>);

    impl SecretStore for Memory {
        fn get(&self, account: &str) -> Result<Option<String>, HarnessError> {
            Ok(self.0.borrow().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> Result<(), HarnessError> {
            self.0.borrow_mut().insert(account.into(), secret.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), HarnessError> {
            self.0.borrow_mut().remove(account);
            Ok(())
        }
        fn place(&self) -> String {
            "memory".into()
        }
    }

    fn entry(kind: &str) -> Entry {
        Entry {
            kind: kind.into(),
            root: None,
            model: Some("deepseek/deepseek-v4.1-flash".into()),
            key_env: None,
            key: None,
        }
    }

    fn providers(list: &[(&str, Entry)]) -> Providers {
        Providers {
            providers: list
                .iter()
                .map(|(id, entry)| ((*id).to_owned(), entry.clone()))
                .collect(),
            ..Providers::default()
        }
    }

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn the_key_comes_from_the_providers_variable_then_the_store_never_from_axh_api_key() {
        let list = providers(&[("openrouter", entry("openrouter"))]);
        let store = Memory::default();
        store
            .set("openrouter@openrouter.ai", " sk-store\n")
            .unwrap();
        let from_store = resolve(
            &list,
            &store,
            None,
            None,
            env(&[("AXH_API_KEY", "sk-host")]),
        )
        .unwrap();
        assert_eq!(from_store.key_from, KeyFrom::Store);
        assert_eq!(
            from_store.endpoint.api_key.as_deref(),
            Some("sk-store"),
            "trimmed, and not AXH_API_KEY"
        );
        assert_eq!(from_store.endpoint.base_url, "https://openrouter.ai/api/v1");
        assert!(from_store.endpoint.versioned);
        let blank = resolve(
            &list,
            &store,
            None,
            None,
            env(&[("OPENROUTER_API_KEY", "  ")]),
        )
        .unwrap();
        assert_eq!(
            blank.key_from,
            KeyFrom::Store,
            "a blank variable counts as unset"
        );
        let from_env = resolve(
            &list,
            &store,
            None,
            Some("other/model"),
            env(&[("OPENROUTER_API_KEY", "sk-env\n")]),
        )
        .unwrap();
        assert_eq!(from_env.key_from, KeyFrom::Env("OPENROUTER_API_KEY".into()));
        assert_eq!(from_env.endpoint.api_key.as_deref(), Some("sk-env"));
        assert_eq!(from_env.endpoint.model, "other/model");
    }

    #[test]
    fn an_entrys_own_key_variable_beats_the_catalogs() {
        let mut own = entry("openrouter");
        own.key_env = Some("MY_OR_KEY".into());
        let list = providers(&[("openrouter", own)]);
        let found = resolve(
            &list,
            &Memory::default(),
            None,
            None,
            env(&[
                ("OPENROUTER_API_KEY", "sk-catalog"),
                ("MY_OR_KEY", "sk-own"),
            ]),
        )
        .unwrap();
        assert_eq!(found.key_from, KeyFrom::Env("MY_OR_KEY".into()));
        assert_eq!(found.endpoint.api_key.as_deref(), Some("sk-own"));
    }

    #[test]
    fn a_stored_key_stays_with_the_host_it_was_given_for() {
        let store = Memory::default();
        let mut moved = entry("openrouter");
        store.set(&account("openrouter", &moved), "sk-1").unwrap();
        moved.root = Some("https://elsewhere.example/v1/".into());
        let list = providers(&[("openrouter", moved)]);
        let err = resolve(&list, &store, None, None, |_| None).unwrap_err();
        assert!(
            err.contains("no key for openrouter at elsewhere.example"),
            "{err}"
        );
        store.set("openrouter@elsewhere.example", "sk-2").unwrap();
        let found = resolve(&list, &store, None, None, |_| None).unwrap();
        assert_eq!(found.endpoint.base_url, "https://elsewhere.example/v1");
        assert_eq!(found.endpoint.api_key.as_deref(), Some("sk-2"));
    }

    #[test]
    fn a_missing_key_a_missing_model_and_an_unclear_default_are_said() {
        let store = Memory::default();
        let none = |_: &str| None;
        let one = providers(&[("openrouter", entry("openrouter"))]);
        assert!(
            resolve(&one, &store, None, None, none)
                .unwrap_err()
                .contains("no key for openrouter")
        );
        let two = providers(&[("openrouter", entry("openrouter")), ("groq", entry("groq"))]);
        assert!(
            resolve(&two, &store, None, None, none)
                .unwrap_err()
                .contains("no default provider")
        );
        assert!(
            resolve(&two, &store, Some("nonesuch"), None, none)
                .unwrap_err()
                .contains("no provider nonesuch")
        );
        let mut local = entry("ollama");
        local.model = None;
        let ollama = providers(&[("ollama", local)]);
        assert!(
            resolve(&ollama, &store, None, None, none)
                .unwrap_err()
                .contains("no model")
        );
        let found = resolve(&ollama, &store, None, Some("qwen3:8b"), none).unwrap();
        assert_eq!(found.key_from, KeyFrom::NotNeeded);
        assert_eq!(found.endpoint.api_key, None);
    }

    #[test]
    fn entries_and_ids_are_checked() {
        assert!(check_id("my-box2").is_ok());
        for bad in ["", "-x", "Open", "a/b", &"x".repeat(41)] {
            assert!(check_id(bad).is_err(), "{bad:?}");
        }
        let mut custom = entry(CUSTOM);
        assert!(custom.check().is_err(), "no root");
        custom.root = Some("http://box:8080/v1".into());
        custom.key = Some(false);
        assert!(custom.check().is_ok());
        assert!(!custom.needs_key());
        custom.key = None;
        let err = custom.check().unwrap_err();
        assert!(
            err.contains("only sent over https"),
            "a key over plain http elsewhere: {err}"
        );
        custom.root = Some("http://localhost:8080/v1".into());
        assert!(custom.check().is_ok(), "this machine is fine");
        for bad in [
            "https://user:pw@box/v1",
            "https://box/v1?key=1",
            "ftp://box/v1",
        ] {
            custom.root = Some(bad.into());
            assert!(custom.check().is_err(), "{bad}");
        }
        assert!(entry("nonesuch").check().is_err());
        let mut flagged = entry("openrouter");
        flagged.key = Some(false);
        assert!(flagged.check().is_err(), "the catalog decides");
    }

    #[test]
    fn the_file_round_trips_and_a_bad_one_is_refused() {
        let home = std::env::temp_dir().join(format!(
            "axh-providers-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        assert!(Providers::load(&home).unwrap().providers.is_empty());
        let mut list = providers(&[("openrouter", entry("openrouter"))]);
        list.default = Some("openrouter".into());
        list.save(&home).unwrap();
        let back = Providers::load(&home).unwrap();
        assert_eq!(back.default.as_deref(), Some("openrouter"));
        assert_eq!(back.providers["openrouter"], entry("openrouter"));
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(home.join(DIR))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "~/.axh is the owner's alone");
        for bad in [
            r#"{"providers":{"Bad":{"kind":"openrouter"}}}"#,
            r#"{"providers":{"x":{"kind":"openrouter","signIn":"chatgpt"}}}"#,
        ] {
            std::fs::write(Providers::path(&home), bad).unwrap();
            assert!(Providers::load(&home).is_err(), "{bad}");
        }
    }
}
