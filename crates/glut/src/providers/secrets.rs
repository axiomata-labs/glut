//! Where a provider's key waits between runs (`docs/plans/glut-providers.md`, P4): the system's secret store, or a
//! file only the owner can read — the owner's choice (`credentialStore`). Never `providers.json`, never a command's
//! environment.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::HarnessError;

/// The secret store's service name; the account is the provider's id.
pub const SERVICE: &str = "glut";
/// The file store, below `~/.glut`.
pub const FILE: &str = "credentials.json";
/// A credentials file larger than this is none.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Which store the owner chose (`credentialStore` in `~/.glut/providers.json`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreChoice {
    /// The system's store where Glut has one, else the file.
    #[default]
    Auto,
    /// The system's store, or an error.
    Keyring,
    /// `~/.glut/credentials.json`, mode 0600.
    File,
}

/// A place secrets are kept, by account — shared by the tasks of a session (an MCP server refreshing its sign-in, T56).
pub trait SecretStore: Send + Sync {
    /// The account's secret; `None` when it has none.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be read.
    fn get(&self, account: &str) -> Result<Option<String>, HarnessError>;
    /// Puts `secret` under `account`, replacing what was there.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be written.
    fn set(&self, account: &str, secret: &str) -> Result<(), HarnessError>;
    /// Removes the account's secret; one that has none is no error.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be written.
    fn delete(&self, account: &str) -> Result<(), HarnessError>;
    /// Where the secrets are, for `glut providers list`.
    fn place(&self) -> String;
}

/// The store `choice` names, for the home folder `home`.
///
/// # Errors
///
/// [`HarnessError::Settings`] for `keyring` on a system Glut has no store for yet.
pub fn open(choice: StoreChoice, home: &Path) -> Result<Box<dyn SecretStore>, HarnessError> {
    let file =
        || Box::new(FileStore::new(home.join(super::DIR).join(FILE))) as Box<dyn SecretStore>;
    match choice {
        StoreChoice::File => Ok(file()),
        StoreChoice::Auto => Ok(system().unwrap_or_else(file)),
        StoreChoice::Keyring => system().ok_or_else(|| HarnessError::Settings {
            path: "credentialStore".into(),
            reason: "this system has no secret store Glut can use yet; choose \"file\"".into(),
        }),
    }
}

#[cfg(target_os = "macos")]
fn system() -> Option<Box<dyn SecretStore>> {
    Some(Box::new(Keychain))
}

#[cfg(not(target_os = "macos"))]
fn system() -> Option<Box<dyn SecretStore>> {
    // Windows' Credential Manager and Linux' Secret Service follow with keys-platforms.md (P4).
    None
}

/// The macOS login Keychain: generic passwords of the service [`SERVICE`].
#[cfg(target_os = "macos")]
pub struct Keychain;

#[cfg(target_os = "macos")]
impl Keychain {
    /// The Security framework's "no such item".
    const NOT_FOUND: i32 = -25300;

    fn fail(err: &security_framework::base::Error) -> HarnessError {
        HarnessError::Settings {
            path: "the Keychain".into(),
            reason: err.to_string(),
        }
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, HarnessError> {
        match security_framework::passwords::get_generic_password(SERVICE, account) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| HarnessError::Settings {
                    path: "the Keychain".into(),
                    reason: format!("the secret of {account} is no text"),
                }),
            Err(err) if err.code() == Self::NOT_FOUND => Ok(None),
            Err(err) => Err(Self::fail(&err)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), HarnessError> {
        security_framework::passwords::set_generic_password(SERVICE, account, secret.as_bytes())
            .map_err(|err| Self::fail(&err))
    }

    fn delete(&self, account: &str) -> Result<(), HarnessError> {
        match security_framework::passwords::delete_generic_password(SERVICE, account) {
            Err(err) if err.code() != Self::NOT_FOUND => Err(Self::fail(&err)),
            _ => Ok(()),
        }
    }

    fn place(&self) -> String {
        format!("the Keychain (service {SERVICE})")
    }
}

/// `~/.glut/credentials.json`: `{"secrets": {"<account>": "<secret>"}}`, readable by the owner alone.
///
/// Written whole into a new file created with mode 0600 and renamed into place — never written first and narrowed
/// after, which left Pi's file open to others for a moment (CVE-2026-54327). A file anyone else may read is refused,
/// not used.
pub struct FileStore {
    path: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Secrets {
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        FileStore { path }
    }

    fn fail(&self, reason: impl Into<String>) -> HarnessError {
        HarnessError::Settings {
            path: self.path.display().to_string(),
            reason: reason.into(),
        }
    }

    fn load(&self) -> Result<Secrets, HarnessError> {
        let file = match super::open_no_follow(&self.path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Secrets::default()),
            Err(err) => return Err(self.fail(err.to_string())),
        };
        let meta = file.metadata().map_err(|err| self.fail(err.to_string()))?;
        if !meta.is_file() {
            return Err(self.fail("not a regular file"));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(self.fail(format!(
                "others may read it (mode {:o}); `chmod 600` it, or remove it and add the keys again",
                meta.permissions().mode() & 0o777
            )));
        }
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| self.fail(err.to_string()))?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(self.fail(format!("larger than {MAX_FILE_BYTES} bytes")));
        }
        serde_json::from_str(&text).map_err(|err| self.fail(err.to_string()))
    }

    fn save(&self, secrets: &Secrets) -> Result<(), HarnessError> {
        let dir = self.path.parent().ok_or_else(|| self.fail("no folder"))?;
        super::make_private_dir(dir).map_err(|err| self.fail(err.to_string()))?;
        let text =
            serde_json::to_string_pretty(secrets).map_err(|err| self.fail(err.to_string()))?;
        super::write_replacing(dir, &self.path, text.as_bytes(), 0o600)
            .map_err(|err| self.fail(err.to_string()))
    }
}

impl SecretStore for FileStore {
    fn get(&self, account: &str) -> Result<Option<String>, HarnessError> {
        Ok(self.load()?.secrets.remove(account))
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), HarnessError> {
        let mut secrets = self.load()?;
        secrets
            .secrets
            .insert(account.to_owned(), secret.to_owned());
        self.save(&secrets)
    }

    fn delete(&self, account: &str) -> Result<(), HarnessError> {
        let mut secrets = self.load()?;
        if secrets.secrets.remove(account).is_some() {
            self.save(&secrets)?;
        }
        Ok(())
    }

    fn place(&self) -> String {
        format!("{} (mode 0600)", self.path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glut-secrets-{tag}-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_file_keeps_replaces_and_forgets_secrets_and_is_the_owners_alone() {
        let store = FileStore::new(scratch("keep").join(FILE));
        assert_eq!(store.get("openrouter").unwrap(), None);
        store.set("openrouter", "sk-1").unwrap();
        store.set("groq", "gsk-2").unwrap();
        store.set("openrouter", "sk-3").unwrap();
        assert_eq!(store.get("openrouter").unwrap().as_deref(), Some("sk-3"));
        let mode = std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        store.delete("openrouter").unwrap();
        store.delete("never-there").unwrap();
        assert_eq!(store.get("openrouter").unwrap(), None);
        assert_eq!(store.get("groq").unwrap().as_deref(), Some("gsk-2"));
    }

    #[test]
    fn a_file_others_may_read_or_a_link_is_refused() {
        let dir = scratch("refuse");
        let store = FileStore::new(dir.join(FILE));
        store.set("openrouter", "sk-1").unwrap();
        std::fs::set_permissions(&store.path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let err = store.get("openrouter").unwrap_err().to_string();
        assert!(err.contains("others may read it"), "{err}");

        let elsewhere = dir.join("elsewhere.json");
        std::fs::write(&elsewhere, r#"{"secrets":{}}"#).unwrap();
        let linked = FileStore::new(dir.join("linked.json"));
        std::os::unix::fs::symlink(&elsewhere, &linked.path).unwrap();
        assert!(linked.get("x").is_err(), "a link is not followed");
    }
}
