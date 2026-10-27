//! Sign-in by subscription (`docs/plans/glut-providers.md`, PV4): a provider the owner signs in to with an account
//! instead of a key — today ChatGPT's plan, through "Sign in with ChatGPT" for open-source apps. The sign-in itself is
//! OAuth's authorization code with PKCE ([`crate::oauth`]); what it leaves is a [`Record`] in the provider's place of
//! the secret store (`<id>@<host>`, the Keychain or the 0600 file), and a run's model asks a [`Credential`] for a
//! bearer token at every call, renewed when it runs out.
//!
//! A refresh token is replaced at every renewal: two Glut windows, or a plan's run beside them, renewing at once would
//! sign each other out. A renewal therefore takes a lock of its own (`~/.glut/locks/<account>.lock`), reads the store
//! again first, and renews only a token nobody renewed meanwhile.

pub mod chatgpt;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::HarnessError;
use crate::providers::Providers;

/// The folder of the renewals' locks, below `~/.glut`.
const LOCKS: &str = "locks";
/// How long before its end an access token counts as spent: a request must not leave with one that runs out on the
/// way.
const MARGIN_SECS: i64 = 120;

/// What a sign-in leaves for one account: the client the provider issued to Glut on this host, who signed in, and the
/// tokens — gone after a sign-out, while the client and the host's id stay for the next sign-in.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// The client id the provider issued (`oaiapp_…`), reused at every sign-in of this account on this host.
    pub client_id: String,
    /// This host's id towards the provider (`ext_agent_host_id`): random, made once, kept.
    pub host_id: String,
    /// The account, as the verified ID token names it (`sub`).
    pub subject: String,
    /// The account's email, shown to the owner and sent as the next sign-in's hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The last ID token: a hint for the next sign-in to the same account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// The bearer token of the calls, good for an hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// What renews it; replaced at every renewal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When the access token runs out, in Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// What the grant allows.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// When the tokens came, in Unix seconds.
    #[serde(default)]
    pub saved_at: i64,
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never a token: an assertion or a log of a record must not print one.
        f.debug_struct("Record")
            .field("client_id", &self.client_id)
            .field("subject", &self.subject)
            .field("signed_in", &self.signed_in())
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Record {
    /// Whether the account is signed in: there is a refresh token to renew with.
    pub fn signed_in(&self) -> bool {
        self.refresh_token.is_some()
    }

    /// The access token while it is good for a call, as far as the provider said when it ends.
    fn fresh_access(&self, now: i64) -> Option<&str> {
        let token = self.access_token.as_deref()?;
        let end = self.expires_at?;
        (end - MARGIN_SECS > now).then_some(token)
    }

    /// The record as the store keeps it.
    fn to_secret(&self) -> Result<String, HarnessError> {
        serde_json::to_string(self).map_err(|err| HarnessError::Protocol(err.to_string()))
    }
}

/// The record kept under `account`, `None` where there is none or it is no record Glut reads.
///
/// # Errors
///
/// [`HarnessError::Settings`] when the store cannot be read.
pub fn load(home: &Path, account: &str) -> Result<Option<Record>, HarnessError> {
    let store = Providers::load(home)?.store(home)?;
    Ok(store
        .get(account)?
        .and_then(|secret| serde_json::from_str(&secret).ok()))
}

/// `record` kept under `account`, replacing what was there.
///
/// # Errors
///
/// [`HarnessError::Settings`] when the store cannot be written.
pub fn save(home: &Path, account: &str, record: &Record) -> Result<(), HarnessError> {
    let store = Providers::load(home)?.store(home)?;
    store.set(account, &record.to_secret()?)
}

/// How long a renewal waits for another one of the same account before it gives up: a renewal takes a second.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
/// How often a waiting renewal tries the lock again.
const LOCK_RETRY: std::time::Duration = std::time::Duration::from_millis(100);

/// The lock of `account`'s record, held while it lives: a renewal, a sign-in and a sign-out each read the record and
/// write it back whole, one at a time across every Glut of the owner.
pub struct Lock(#[allow(dead_code)] std::fs::File);

impl Lock {
    /// The lock of `account`'s record, waited for at most [`LOCK_WAIT`] beside the screen.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the lock cannot be opened or another holds it too long.
    pub async fn take(home: &Path, account: &str) -> Result<Lock, HarnessError> {
        let dir = home.join(".glut").join(LOCKS);
        // The account is `<id>@<host>`: a provider id and a host, neither of which holds a path separator, but a
        // file's name is made of its safe characters alone.
        let name: String = account
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "@.-_".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("{name}.lock"));
        let fail = |reason: String| HarnessError::Settings {
            path: "~/.glut/locks".into(),
            reason,
        };
        let file = tokio::task::spawn_blocking(move || {
            use std::os::unix::fs::OpenOptionsExt as _;
            crate::providers::make_private_dir(&dir)?;
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
                .open(&path)
        })
        .await
        .map_err(|err| fail(err.to_string()))?
        .map_err(|err| fail(err.to_string()))?;
        let began = std::time::Instant::now();
        loop {
            match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(Lock(file)),
                Err(rustix::io::Errno::WOULDBLOCK) if began.elapsed() < LOCK_WAIT => {
                    tokio::time::sleep(LOCK_RETRY).await;
                }
                Err(rustix::io::Errno::WOULDBLOCK) => {
                    return Err(fail(
                        "another Glut holds the sign-in's lock too long".into(),
                    ));
                }
                Err(err) => return Err(fail(err.to_string())),
            }
        }
    }
}

/// A signed-in provider's bearer token for a run's calls: the access token while it is good, renewed when it is
/// spent — the record read again under the lock first, so a token another process renewed meanwhile is taken, not
/// renewed twice.
#[derive(Clone)]
pub struct Credential {
    inner: Arc<Inner>,
}

struct Inner {
    home: PathBuf,
    /// The provider's id, for what the owner is told.
    provider: String,
    account: String,
    server: chatgpt::Server,
    held: tokio::sync::Mutex<Option<Record>>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the tokens.
        f.debug_struct("Credential")
            .field("account", &self.inner.account)
            .finish_non_exhaustive()
    }
}

impl Credential {
    /// The credential of provider `provider`, kept under `account` in `home`'s store.
    pub fn new(home: &Path, provider: &str, account: &str) -> Credential {
        Credential::at(home, provider, account, chatgpt::Server::openai())
    }

    /// [`Self::new`] against `server` (a test's).
    pub(crate) fn at(
        home: &Path,
        provider: &str,
        account: &str,
        server: chatgpt::Server,
    ) -> Credential {
        Credential {
            inner: Arc::new(Inner {
                home: home.to_path_buf(),
                provider: provider.to_owned(),
                account: account.to_owned(),
                server,
                held: tokio::sync::Mutex::new(None),
            }),
        }
    }

    /// A bearer token for the next call.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Protocol`] when the account is not signed in or the provider refused the renewal (the owner
    /// signs in again); [`HarnessError::Transport`] or [`HarnessError::Settings`] when the provider or the store
    /// cannot be reached.
    pub async fn bearer(&self) -> Result<String, HarnessError> {
        self.fresh(None).await
    }

    /// A bearer token other than `refused`, which the API turned away though its time said it holds (revoked, or a
    /// clock off): renewed unless another window renewed it meanwhile.
    ///
    /// # Errors
    ///
    /// As [`Self::bearer`].
    pub async fn instead_of(&self, refused: &str) -> Result<String, HarnessError> {
        self.fresh(Some(refused.to_owned())).await
    }

    async fn fresh(&self, refused: Option<String>) -> Result<String, HarnessError> {
        let usable = |record: &Record| {
            record
                .fresh_access(crate::oauth::now())
                .filter(|token| refused.as_deref() != Some(*token))
                .map(str::to_owned)
        };
        let mut held = self.inner.held.lock().await;
        if let Some(token) = held.as_ref().and_then(usable) {
            return Ok(token);
        }
        // A task of its own: once the renewal is asked for, the old refresh token is spent — a turn stopped meanwhile
        // must not take the new one with it unsaved.
        let inner = Arc::clone(&self.inner);
        let refused_too = refused.clone();
        let renewal = tokio::spawn(async move {
            let _lock = Lock::take(&inner.home, &inner.account).await?;
            let record = load(&inner.home, &inner.account)?
                .filter(Record::signed_in)
                .ok_or_else(|| not_signed_in(&inner.provider))?;
            // Renewed by another window or a plan's run while this one waited: theirs is good.
            let theirs = record
                .fresh_access(crate::oauth::now())
                .is_some_and(|token| refused_too.as_deref() != Some(token));
            if theirs {
                return Ok(record);
            }
            let renewed = chatgpt::renew(&inner.server, &record)
                .await
                .map_err(|failure| match failure {
                    chatgpt::Failure::SignInAgain(_) => not_signed_in(&inner.provider),
                    chatgpt::Failure::Other(reason) => HarnessError::Transport(reason),
                })?;
            // The new refresh token is the only good one now: the store asked once more where it failed, and the
            // run goes on with it either way.
            if save(&inner.home, &inner.account, &renewed).is_err() {
                let _ = save(&inner.home, &inner.account, &renewed);
            }
            Ok::<_, HarnessError>(renewed)
        });
        let record = renewal
            .await
            .map_err(|err| HarnessError::Transport(err.to_string()))??;
        let token = record.access_token.clone().unwrap_or_default();
        *held = Some(record);
        Ok(token)
    }
}

/// What the owner is told when a provider wants them to sign in (again).
pub fn not_signed_in(provider: &str) -> HarnessError {
    HarnessError::Protocol(format!(
        "{provider} is not signed in: glut providers login {provider} (or /login in Glut) signs in"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// A token endpoint that renews: each answer a new access token, counted.
    async fn renewing() -> (chatgpt::Server, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = vec![0; 8 * 1024];
                let _ = socket.read(&mut buffer).await;
                let n = seen.fetch_add(1, Ordering::SeqCst) + 1;
                let answer = serde_json::json!({"access_token": format!("fresh-{n}"),
                    "refresh_token": format!("refresh-{n}"), "token_type": "Bearer", "expires_in": 3600})
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                     connection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        let server = chatgpt::Server {
            issuer: "https://auth.openai.com".into(),
            authorize: format!("{root}/authorize"),
            token: format!("{root}/token"),
            revoke: format!("{root}/revoke"),
            jwks: format!("{root}/jwks"),
        };
        (server, count)
    }

    #[tokio::test]
    async fn a_spent_token_is_renewed_once_kept_and_one_renewed_elsewhere_is_taken() {
        let home = std::env::temp_dir().join(format!(
            "glut-credential-{}-{}",
            std::process::id(),
            crate::oauth::random_text().unwrap()
        ));
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        std::fs::write(
            home.join(".glut").join("providers.json"),
            r#"{"credentialStore": "file", "providers": {}}"#,
        )
        .unwrap();
        let account = "chatgpt@api.openai.com";
        let now = crate::oauth::now();
        save(
            &home,
            account,
            &Record {
                client_id: "oaiapp_1".into(),
                access_token: Some("old".into()),
                refresh_token: Some("refresh-0".into()),
                expires_at: Some(now - 10),
                ..Record::default()
            },
        )
        .unwrap();
        let (server, count) = renewing().await;
        let credential = Credential::at(&home, "chatgpt", account, server.clone());
        assert_eq!(credential.bearer().await.unwrap(), "fresh-1");
        assert_eq!(
            credential.bearer().await.unwrap(),
            "fresh-1",
            "held while it is good"
        );
        let kept = load(&home, account).unwrap().unwrap();
        assert_eq!(
            kept.refresh_token.as_deref(),
            Some("refresh-1"),
            "the rotated token kept with it"
        );
        // Another window renewed meanwhile: its token is taken, not renewed twice.
        save(
            &home,
            account,
            &Record {
                access_token: Some("theirs".into()),
                expires_at: Some(now + 3600),
                ..kept.clone()
            },
        )
        .unwrap();
        let other = Credential::at(&home, "chatgpt", account, server);
        assert_eq!(other.bearer().await.unwrap(), "theirs");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Signed out: the owner is told to sign in.
        save(&home, account, &Record::default()).unwrap();
        let out = Credential::new(&home, "chatgpt", account);
        assert!(
            out.bearer()
                .await
                .unwrap_err()
                .to_string()
                .contains("login chatgpt")
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_record_is_fresh_until_shortly_before_its_end_and_signed_in_while_it_can_renew() {
        let record = Record {
            access_token: Some("a".into()),
            expires_at: Some(1_000),
            refresh_token: Some("r".into()),
            ..Record::default()
        };
        assert_eq!(record.fresh_access(1_000 - MARGIN_SECS - 1), Some("a"));
        assert_eq!(
            record.fresh_access(1_000 - MARGIN_SECS),
            None,
            "about to run out: renewed first"
        );
        assert!(record.signed_in());
        let out = Record {
            refresh_token: None,
            ..record.clone()
        };
        assert!(!out.signed_in());
        let debug = format!(
            "{:?}",
            Credential::new(Path::new("/h"), "chatgpt", "chatgpt@api.openai.com")
        );
        assert!(!debug.contains("access") && debug.contains("chatgpt@api.openai.com"));
    }
}
