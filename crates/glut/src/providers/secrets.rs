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

/// Whether this build reads secrets from the file where the system has a store of its own (T87, owner 2026-10-30):
/// on macOS only a build with the feature `dev-keys` (`scripts/install.sh`, the bench) — every new build is a new
/// program to the Keychain, which asks again and leaves an unattended run hanging while Glut is developed; a build
/// that goes to others keeps the Keychain as the one place of truth. Elsewhere the file is the only store yet. Tests
/// keep theirs in a scratch home's file, never in the owner's Keychain.
pub const FILE_ALLOWED: bool = cfg!(any(not(target_os = "macos"), feature = "dev-keys", test));

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
    /// Every account that has a secret here, for moving them to another store (T87).
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be read.
    fn accounts(&self) -> Result<Vec<String>, HarnessError>;
}

/// Whether the owner chose the file and this build reads the system's store instead ([`FILE_ALLOWED`]).
pub fn file_ignored(choice: StoreChoice) -> bool {
    ignored_where(choice, FILE_ALLOWED)
}

/// [`file_ignored`] with the build's allowance handed in, so a test sees both builds.
fn ignored_where(choice: StoreChoice, allowed: bool) -> bool {
    choice == StoreChoice::File && !allowed
}

/// Copies every secret of `from` into `to` and returns how many (T87): the source keeps them, so a failure loses
/// nothing — [`forget_all`] empties it once the new place is the one in use.
///
/// # Errors
///
/// [`HarnessError::Settings`] when either store cannot be read or written.
pub fn copy_secrets(from: &dyn SecretStore, to: &dyn SecretStore) -> Result<usize, HarnessError> {
    let mut copied = 0;
    for account in from.accounts()? {
        if let Some(secret) = from.get(&account)? {
            to.set(&account, &secret)?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Takes every secret out of `store`.
///
/// # Errors
///
/// [`HarnessError::Settings`] when the store cannot be read or written.
pub fn forget_all(store: &dyn SecretStore) -> Result<(), HarnessError> {
    store
        .accounts()?
        .iter()
        .try_for_each(|account| store.delete(account))
}

/// The store `choice` names, for the home folder `home`.
///
/// # Errors
///
/// [`HarnessError::Settings`] for `keyring` on a system Glut has no store for yet.
pub fn open(choice: StoreChoice, home: &Path) -> Result<Box<dyn SecretStore>, HarnessError> {
    open_where(choice, home, FILE_ALLOWED)
}

/// [`open`] with the build's allowance handed in.
fn open_where(
    choice: StoreChoice,
    home: &Path,
    allowed: bool,
) -> Result<Box<dyn SecretStore>, HarnessError> {
    let file =
        || Box::new(FileStore::new(home.join(super::DIR).join(FILE))) as Box<dyn SecretStore>;
    match choice {
        StoreChoice::File if ignored_where(choice, allowed) => Ok(system().unwrap_or_else(file)),
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

    fn accounts(&self) -> Result<Vec<String>, HarnessError> {
        use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
        // Attributes only: the names come without the secrets, so listing them asks the owner nothing.
        let found = match ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(SERVICE)
            .load_attributes(true)
            .limit(Limit::All)
            .search()
        {
            Ok(found) => found,
            Err(err) if err.code() == Self::NOT_FOUND => return Ok(Vec::new()),
            Err(err) => return Err(Self::fail(&err)),
        };
        let mut accounts: Vec<String> = found
            .iter()
            .filter_map(|item| item.simplify_dict()?.remove("acct"))
            .collect();
        accounts.sort();
        accounts.dedup();
        Ok(accounts)
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

impl FileStore {
    /// The file's lock, held while one change reads and writes it whole: two writers at once (an MCP token, a
    /// subscription's renewal, `glut providers add`) would otherwise each write back what the other did not see.
    fn lock(&self) -> Result<std::fs::File, HarnessError> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let dir = self.path.parent().unwrap_or(Path::new("."));
        super::make_private_dir(dir).map_err(|err| self.fail(err.to_string()))?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(dir.join(".credentials.lock"))
            .map_err(|err| self.fail(err.to_string()))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
            .map_err(|err| self.fail(err.to_string()))?;
        Ok(file)
    }
}

impl SecretStore for FileStore {
    fn get(&self, account: &str) -> Result<Option<String>, HarnessError> {
        Ok(self.load()?.secrets.remove(account))
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), HarnessError> {
        let _held = self.lock()?;
        let mut secrets = self.load()?;
        secrets
            .secrets
            .insert(account.to_owned(), secret.to_owned());
        self.save(&secrets)
    }

    fn delete(&self, account: &str) -> Result<(), HarnessError> {
        let _held = self.lock()?;
        let mut secrets = self.load()?;
        if secrets.secrets.remove(account).is_some() {
            self.save(&secrets)?;
        }
        Ok(())
    }

    fn place(&self) -> String {
        format!("{} (mode 0600)", self.path.display())
    }

    fn accounts(&self) -> Result<Vec<String>, HarnessError> {
        Ok(self.load()?.secrets.into_keys().collect())
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

    #[test]
    fn secrets_are_copied_whole_and_the_source_kept_until_forgotten() {
        let dir = scratch("move");
        let (file, other) = (
            FileStore::new(dir.join(FILE)),
            FileStore::new(dir.join("other.json")),
        );
        file.set("openrouter@openrouter.ai", "sk-1").unwrap();
        file.set("mcp-oauth", "t-2").unwrap();
        assert_eq!(copy_secrets(&file, &other).unwrap(), 2);
        assert_eq!(
            copy_secrets(&FileStore::new(dir.join("none.json")), &other).unwrap(),
            0
        );
        assert_eq!(other.get("mcp-oauth").unwrap().as_deref(), Some("t-2"));
        assert_eq!(
            other.get("openrouter@openrouter.ai").unwrap().as_deref(),
            Some("sk-1")
        );
        assert_eq!(file.accounts().unwrap().len(), 2, "the source kept");
        forget_all(&file).unwrap();
        assert!(file.accounts().unwrap().is_empty());
    }

    #[test]
    fn the_file_is_read_only_where_this_build_allows_it() {
        assert_eq!(file_ignored(StoreChoice::File), !FILE_ALLOWED);
        assert!(!file_ignored(StoreChoice::Auto) && !file_ignored(StoreChoice::Keyring));
        let store = open(StoreChoice::File, &scratch("open")).unwrap();
        assert_eq!(
            store.place().contains(FILE),
            FILE_ALLOWED,
            "{}",
            store.place()
        );
    }
}
