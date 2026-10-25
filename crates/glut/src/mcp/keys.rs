//! What a hosted MCP server is signed in with (`docs/plans/glut-tui.md`, T56): an OAuth grant, or a token the owner
//! typed for a server without OAuth (GitHub's and GitLab's personal tokens) — both in the secret store the owner
//! chose for the providers' keys (the Keychain or `~/.glut/credentials.json`), never in `mcp.json`, never in a
//! variable.
//!
//! A grant is bound to the server's exact URL, a token to its host — the account names them, the way a provider's
//! key is bound to its host: an entry of the same name that points elsewhere finds nothing instead of sending what was
//! given for another server.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::HarnessError;
use crate::providers::secrets::SecretStore;

/// The account of a grant: `mcp-oauth:<name>@<url>`.
const OAUTH: &str = "mcp-oauth:";
/// The account of a typed token: `mcp-token:<name>@<host>`.
const TOKEN: &str = "mcp-token:";
/// The account of a registered client's secret (`"oauth": {"clientId"}`): `mcp-client:<name>@<host>`.
const CLIENT: &str = "mcp-client:";

/// An OAuth grant as Glut keeps it: the tokens, and everything a refresh and a revocation need, so neither has to
/// discover the server again.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When the access token runs out, in Unix seconds; `None`: the server did not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub client_id: String,
    /// A confidential client's secret: the one the registration handed out, or the owner's for their own client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,
    /// The resource the tokens are for (RFC 8707), sent again with a refresh.
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl Grant {
    /// How long before its end an access token counts as spent: a request must not leave with one that runs out on
    /// the way.
    const MARGIN_SECS: i64 = 60;

    /// Whether the access token is spent (or about to be), as far as the server said when it ends.
    pub fn spent(&self, now: i64) -> bool {
        self.expires_at
            .is_some_and(|end| end - Self::MARGIN_SECS <= now)
    }
}

/// What a server is signed in with.
#[derive(Clone, PartialEq)]
pub enum Signed {
    /// A token the owner typed: sent as `Authorization: Bearer`.
    Token(String),
    /// An OAuth grant: its access token is sent, refreshed when spent.
    OAuth(Grant),
}

/// A grant's tokens and secret never in a debug print: one `{:?}` in a log must not hand them out.
impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("client_id", &self.client_id)
            .field("token_endpoint", &self.token_endpoint)
            .field("resource", &self.resource)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Signed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Signed::Token(_) => f.write_str("Token(…)"),
            Signed::OAuth(grant) => f.debug_tuple("OAuth").field(grant).finish(),
        }
    }
}

/// The secret store the sign-ins are kept in, shared by the session's servers; none: nothing is looked up (a
/// caller with a store of its own, the tests).
#[derive(Clone, Default)]
pub struct Keys {
    store: Option<Arc<dyn SecretStore>>,
}

impl Keys {
    /// No store: a server is reached without a sign-in.
    pub fn none() -> Self {
        Keys::default()
    }

    /// The store `store`.
    pub fn of(store: Box<dyn SecretStore>) -> Self {
        Keys {
            store: Some(Arc::from(store)),
        }
    }

    /// The owner's store, as `~/.glut/providers.json` names it (`credentialStore`).
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the providers file is broken or the store cannot be opened.
    pub fn owner(home: &std::path::Path) -> Result<Self, HarnessError> {
        let providers = crate::providers::Providers::load(home)?;
        Ok(Keys::of(providers.store(home)?))
    }

    /// Where the sign-ins are kept, for the owner to read.
    pub fn place(&self) -> Option<String> {
        self.store.as_ref().map(|store| store.place())
    }

    /// What the server `name` at `url` is signed in with: its grant first, then a typed token.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be read; [`HarnessError::Mcp`] for a grant that is no grant.
    pub fn signed(&self, name: &str, url: &str) -> Result<Option<Signed>, HarnessError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        if let Some(text) = store.get(&oauth_account(name, url))? {
            let grant = serde_json::from_str(&text).map_err(|_| HarnessError::Mcp {
                server: name.to_owned(),
                message: "its kept sign-in cannot be read; sign in again".into(),
            })?;
            return Ok(Some(Signed::OAuth(grant)));
        }
        Ok(store
            .get(&token_account(name, url))?
            .filter(|token| !token.is_empty())
            .map(Signed::Token))
    }

    /// Keeps `grant` for the server, over what was kept before.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when there is no store or it cannot be written.
    pub fn keep_grant(&self, name: &str, url: &str, grant: &Grant) -> Result<(), HarnessError> {
        let text = serde_json::to_string(grant).map_err(|err| HarnessError::Mcp {
            server: name.to_owned(),
            message: format!("its sign-in cannot be written down: {err}"),
        })?;
        self.store()?.set(&oauth_account(name, url), &text)
    }

    /// Keeps a token the owner typed for the server, over what was kept before.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when there is no store or it cannot be written.
    pub fn keep_token(&self, name: &str, url: &str, token: &str) -> Result<(), HarnessError> {
        self.store()?.set(&token_account(name, url), token)
    }

    /// The secret of the client the owner registered for the server (`"oauth": {"clientId"}`), if they gave one.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be read.
    pub fn client_secret(&self, name: &str, url: &str) -> Result<Option<String>, HarnessError> {
        match &self.store {
            Some(store) => Ok(store
                .get(&client_account(name, url))?
                .filter(|secret| !secret.is_empty())),
            None => Ok(None),
        }
    }

    /// Keeps the secret of the owner's client for the server.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when there is no store or it cannot be written.
    pub fn keep_client_secret(
        &self,
        name: &str,
        url: &str,
        secret: &str,
    ) -> Result<(), HarnessError> {
        self.store()?.set(&client_account(name, url), secret)
    }

    /// Forgets what the server is signed in with — the grant and the token; the owner's client's secret stays for the
    /// next sign-in ([`Self::forget_client_secret`] takes it) — and hands back the grant, for a revocation at the
    /// server.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be written.
    pub fn forget(&self, name: &str, url: &str) -> Result<Option<Grant>, HarnessError> {
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let grant = match self.signed(name, url) {
            Ok(Some(Signed::OAuth(grant))) => Some(grant),
            _ => None,
        };
        store.delete(&oauth_account(name, url))?;
        store.delete(&token_account(name, url))?;
        Ok(grant)
    }

    /// Forgets the secret of the owner's client for the server (a server removed).
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the store cannot be written.
    pub fn forget_client_secret(&self, name: &str, url: &str) -> Result<(), HarnessError> {
        match &self.store {
            Some(store) => store.delete(&client_account(name, url)),
            None => Ok(()),
        }
    }

    fn store(&self) -> Result<&Arc<dyn SecretStore>, HarnessError> {
        self.store.as_ref().ok_or_else(|| HarnessError::Settings {
            path: "credentialStore".into(),
            reason: "this session keeps no sign-ins".into(),
        })
    }
}

fn oauth_account(name: &str, url: &str) -> String {
    format!("{OAUTH}{name}@{url}")
}

fn token_account(name: &str, url: &str) -> String {
    format!("{TOKEN}{name}@{}", host_of(url))
}

fn client_account(name: &str, url: &str) -> String {
    format!("{CLIENT}{name}@{}", host_of(url))
}

/// The host (and port) of `url`, which a token is bound to; the whole text where it is no URL.
pub fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            let host = parsed.host_str()?.to_owned();
            Some(match parsed.port() {
                Some(port) => format!("{host}:{port}"),
                None => host,
            })
        })
        // No URL: a server's own words, cut and made printable before they reach a message.
        .unwrap_or_else(|| crate::permissions::printable(&url.chars().take(80).collect::<String>()))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    /// A store in memory.
    #[derive(Default)]
    pub(crate) struct Memory(pub(crate) Mutex<BTreeMap<String, String>>);

    impl SecretStore for Memory {
        fn get(&self, account: &str) -> Result<Option<String>, HarnessError> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> Result<(), HarnessError> {
            self.0.lock().unwrap().insert(account.into(), secret.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), HarnessError> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
        fn place(&self) -> String {
            "memory".into()
        }
    }

    pub(crate) fn grant(access: &str) -> Grant {
        Grant {
            access_token: access.into(),
            refresh_token: Some("r-1".into()),
            expires_at: Some(1_000),
            client_id: "c-1".into(),
            client_secret: None,
            token_endpoint: "https://auth.example/token".into(),
            revocation_endpoint: None,
            resource: "https://mcp.example/mcp".into(),
            scope: None,
        }
    }

    #[test]
    fn a_grant_is_bound_to_the_exact_url_and_a_token_to_the_host() {
        let keys = Keys::of(Box::new(Memory::default()));
        keys.keep_grant("notion", "https://mcp.notion.com/mcp", &grant("a-1"))
            .unwrap();
        assert!(matches!(
            keys.signed("notion", "https://mcp.notion.com/mcp").unwrap(),
            Some(Signed::OAuth(grant)) if grant.access_token == "a-1"
        ));
        assert_eq!(
            keys.signed("notion", "https://evil.example/mcp").unwrap(),
            None,
            "the same name elsewhere gets nothing"
        );
        assert_eq!(
            keys.signed("other", "https://mcp.notion.com/mcp").unwrap(),
            None
        );
        keys.keep_token("github", "https://api.githubcopilot.com/mcp/", "ghp_1")
            .unwrap();
        assert_eq!(
            keys.signed("github", "https://api.githubcopilot.com/mcp/x")
                .unwrap(),
            Some(Signed::Token("ghp_1".into())),
            "a token holds for its host"
        );
        assert_eq!(
            keys.signed("github", "https://api.githubcopilot.com:8443/mcp/")
                .unwrap(),
            None
        );
        let forgotten = keys.forget("notion", "https://mcp.notion.com/mcp").unwrap();
        assert_eq!(
            forgotten.map(|grant| grant.access_token).as_deref(),
            Some("a-1")
        );
        assert_eq!(
            keys.signed("notion", "https://mcp.notion.com/mcp").unwrap(),
            None
        );
        assert_eq!(
            Keys::none().signed("github", "https://x.example").unwrap(),
            None
        );
    }

    #[test]
    fn a_grant_is_spent_a_minute_before_its_end() {
        let grant = grant("a");
        assert!(!grant.spent(900));
        assert!(grant.spent(941));
        assert!(
            !Grant {
                expires_at: None,
                ..grant
            }
            .spent(i64::MAX)
        );
    }
}
