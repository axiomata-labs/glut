//! `/mcp` (T56): the session's MCP servers and how each stands, signing in to a hosted one
//! and out again, adding a server to the owner's `~/.glut/mcp.json` and removing it — the same as `glut mcp`. The work
//! runs beside the screen; what it comes to arrives as [`Step`]s. Glut never opens a browser unasked: only the
//! owner's `/mcp login` does, after the note that names where they will sign in.

use std::sync::Arc;

use glut::mcp::config::{self, Adding, Origin, ServerConfig, ServerEntry};
use glut::mcp::oauth::{self, Begun, SignedOut};
use glut::mcp::{Keys, Servers, State};
use tokio::sync::{mpsc, oneshot};

use crate::session::{Parts, Report};
use crate::transcript::Tone;

/// What a `/mcp` command came to, for the screen.
pub enum Step {
    /// A note for the stream.
    Note(String, Tone),
    /// The owner types a token for the server, hidden: it takes no OAuth Glut can use.
    AskToken { name: String, host: String },
    /// The owner types the secret of their own client (`"oauth": {"clientId"}`), or none; the sign-in then begins.
    AskClientSecret { name: String, client: String },
    /// The address to sign in at is shown: the owner's ⏎ opens it in this machine's browser, Esc leaves it.
    Open { address: String },
    /// Sign-in `id` waits for the browser; an address the owner pastes back (`/mcp login <name> <address>`) goes
    /// here.
    Waiting {
        name: String,
        id: u64,
        paste: oneshot::Sender<String>,
    },
    /// Sign-in `id` of `name` ended: nothing waits for its paste any more (a newer one of the same server still may).
    Ended { name: String, id: u64 },
}

/// The number the next sign-in is told by.
static SIGN_INS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The number the next sign-in is told by — an MCP server's or a subscription's (PV4), one count for both.
pub fn next_sign_in() -> u64 {
    SIGN_INS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// What a hidden entry is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretFor {
    Token {
        name: String,
    },
    ClientSecret {
        name: String,
    },
    /// A provider's new key (`/provider key [id]`, T70): kept under `account` — the entry around it unchanged.
    Key {
        id: String,
        account: String,
    },
}

/// A secret the owner types, hidden (T56): a server's token, a client's secret.
pub struct Secret {
    /// What is asked, shown before the hidden text.
    pub prompt: String,
    pub typed: String,
    pub purpose: SecretFor,
    /// When it was asked: a key typed for the input in the moment it came up is not the secret's.
    pub opened: std::time::Instant,
}

/// The address of a sign-in, waiting for the owner's word to open it in the browser.
pub struct Opening {
    pub address: String,
    pub opened: std::time::Instant,
}

/// The dots the hidden text shows once anything is typed: that a paste landed, never how long the secret is.
pub const DOTS: &str = "••••••••";

fn note(reports: &mpsc::UnboundedSender<Report>, text: String, tone: Tone) {
    let _ = reports.send(Report::Mcp(Step::Note(text, tone)));
}

fn keys(parts: &Parts) -> Result<Keys, String> {
    Keys::owner(&parts.home).map_err(|err| err.to_string())
}

/// The session's servers, each with how it stands.
pub fn listing(servers: &Servers) -> String {
    let listed = servers.list();
    if listed.is_empty() {
        return tr!(mcp_none).to_owned();
    }
    let mut lines = vec![tr!(mcp_head).to_owned()];
    for server in listed {
        let state = match &server.state {
            State::Starting => tr!(mcp_starting).to_owned(),
            State::Connected { tools } => tr!(mcp_connected, count = tools),
            State::SignIn => tr!(mcp_wants, name = server.entry.name),
            State::Failed(reason) => tr!(mcp_failed, reason = reason),
            State::Refused => tr!(mcp_refused, name = server.entry.name),
        };
        let what = server.entry.describe();
        let what = what.lines().next().unwrap_or_default();
        let origin = match server.entry.origin {
            Origin::Owner => String::new(),
            Origin::Project => format!(" ({})", tr!(mcp_project)),
        };
        lines.push(format!("  {}  {what}{origin} — {state}", server.entry.name));
    }
    crate::transcript::clean(&lines.join("\n"))
}

/// The URL of a hosted server the owner can sign in to; a refusal says why not.
fn hosted(entry: &ServerEntry) -> Result<String, String> {
    match &entry.config {
        ServerConfig::Stdio { .. } => Err(tr!(mcp_stdio_no_login, name = entry.name)),
        ServerConfig::Http { headers, .. }
            if headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case("authorization")) =>
        {
            Err(tr!(mcp_header_login, name = entry.name))
        }
        ServerConfig::Http { url, .. } => Ok(url.clone()),
    }
}

/// The server `name` of the session, or the note why there is none — or why it is not the owner's to sign in to
/// yet (a project's server not approved).
fn entry(parts: &Parts, name: &str) -> Result<ServerEntry, String> {
    let listed = parts
        .tools
        .servers()
        .list()
        .into_iter()
        .find(|server| server.entry.name == name)
        .ok_or_else(|| tr!(mcp_no_server, name = name))?;
    if listed.state == State::Refused {
        return Err(tr!(mcp_refused, name = name));
    }
    Ok(listed.entry)
}

/// `/mcp login <name>`: a sign-in begun beside the screen — the client's secret asked first where the owner's own
/// client has none kept, a token where the server takes no OAuth.
pub fn login(parts: Arc<Parts>, name: &str, reports: mpsc::UnboundedSender<Report>) {
    let (entry, url) = match entry(&parts, name).and_then(|entry| Ok((hosted(&entry)?, entry))) {
        Ok((url, entry)) => (entry, url),
        Err(reason) => return note(&reports, reason, Tone::Quiet),
    };
    let keys = match keys(&parts) {
        Ok(keys) => keys,
        Err(reason) => return note(&reports, tr!(mcp_error, reason = reason), Tone::Fault),
    };
    if let ServerConfig::Http {
        oauth: Some(client),
        ..
    } = &entry.config
        && keys.client_secret(name, &url).ok().flatten().is_none()
    {
        let _ = reports.send(Report::Mcp(Step::AskClientSecret {
            name: name.to_owned(),
            client: client.client_id.clone(),
        }));
        return;
    }
    begin(parts, entry, url, keys, reports);
}

/// The sign-in itself, once nothing more is to be asked first.
fn begin(
    parts: Arc<Parts>,
    entry: ServerEntry,
    url: String,
    keys: Keys,
    reports: mpsc::UnboundedSender<Report>,
) {
    tokio::spawn(async move {
        let name = entry.name.clone();
        let sign_in = match oauth::begin(&name, &entry.config, &keys).await {
            Ok(Begun::Open) => return note(&reports, tr!(mcp_open, name = name), Tone::Quiet),
            Ok(Begun::Token { registered_only }) => {
                if registered_only {
                    note(&reports, tr!(mcp_registered_only, name = name), Tone::Quiet);
                }
                let _ = reports.send(Report::Mcp(Step::AskToken {
                    host: glut::mcp::keys::host_of(&url),
                    name,
                }));
                return;
            }
            Ok(Begun::SignIn(sign_in)) => sign_in,
            Err(err) => {
                return note(
                    &reports,
                    tr!(mcp_sign_in_failed, name = name, reason = err),
                    Tone::Fault,
                );
            }
        };
        note(
            &reports,
            tr!(
                mcp_sign_in_at,
                name = name,
                host = sign_in.host(),
                address = sign_in.address()
            ),
            Tone::Heed,
        );
        if let Some(host) = sign_in.token_host() {
            note(
                &reports,
                tr!(mcp_token_elsewhere, name = name, host = host),
                Tone::Heed,
            );
        }
        // Nothing opens by itself: the owner reads where they will sign in, then says so — over SSH they open the
        // address where they sit.
        if oauth::can_open() {
            let _ = reports.send(Report::Mcp(Step::Open {
                address: sign_in.address().to_owned(),
            }));
        } else {
            note(&reports, tr!(mcp_paste_back, name = name), Tone::Quiet);
        }
        let id = SIGN_INS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (paste, pasted) = oneshot::channel();
        let _ = reports.send(Report::Mcp(Step::Waiting {
            name: name.clone(),
            id,
            paste,
        }));
        let finished = sign_in.finish(async { pasted.await.ok() }).await;
        let _ = reports.send(Report::Mcp(Step::Ended {
            name: name.clone(),
            id,
        }));
        match finished.and_then(|grant| keys.keep_grant(&name, &url, &grant)) {
            Ok(()) => join(&parts, entry, &keys, &reports).await,
            Err(err) => note(
                &reports,
                tr!(mcp_sign_in_failed, name = name, reason = err),
                Tone::Fault,
            ),
        }
    });
}

/// A server signed in, connected at once: its tools join the session's.
async fn join(
    parts: &Parts,
    entry: ServerEntry,
    keys: &Keys,
    reports: &mpsc::UnboundedSender<Report>,
) {
    let name = entry.name.clone();
    match parts.tools.servers().connect(entry, keys).await {
        State::Connected { tools } => note(
            reports,
            tr!(mcp_signed_in, name = name, count = tools),
            Tone::Fine,
        ),
        State::SignIn => note(reports, tr!(mcp_wants_note, name = name), Tone::Heed),
        State::Failed(reason) => note(
            reports,
            tr!(mcp_not_up, name = name, reason = reason),
            Tone::Fault,
        ),
        State::Starting | State::Refused => {}
    }
}

/// What the owner typed hidden: a token kept and the server connected with it, or the client's secret kept and the
/// sign-in begun.
pub fn secret_given(
    parts: Arc<Parts>,
    purpose: SecretFor,
    typed: String,
    reports: mpsc::UnboundedSender<Report>,
) {
    let name = match &purpose {
        SecretFor::Token { name } | SecretFor::ClientSecret { name } => name.clone(),
        // `/provider key` never comes here: the screen sends it to `key::change` (T70).
        SecretFor::Key { id, .. } => id.clone(),
    };
    let found = entry(&parts, &name).and_then(|entry| Ok((hosted(&entry)?, entry)));
    let ((url, entry), keys) = match found.and_then(|found| Ok((found, keys(&parts)?))) {
        Ok(found) => found,
        Err(reason) => return note(&reports, reason, Tone::Quiet),
    };
    let typed = typed.trim().to_owned();
    if typed.chars().any(|c| c.is_control() || c == ' ') {
        return note(&reports, tr!(secret_odd).to_owned(), Tone::Fault);
    }
    match purpose {
        SecretFor::Token { .. } => {
            if typed.is_empty() {
                return note(&reports, tr!(secret_none).to_owned(), Tone::Quiet);
            }
            if let Err(err) = keys.keep_token(&name, &url, &typed) {
                return note(&reports, tr!(mcp_error, reason = err), Tone::Fault);
            }
            tokio::spawn(async move { join(&parts, entry, &keys, &reports).await });
        }
        SecretFor::ClientSecret { .. } => {
            // Empty: a public client — nothing to keep, and the next sign-in asks again.
            if !typed.is_empty()
                && let Err(err) = keys.keep_client_secret(&name, &url, &typed)
            {
                return note(&reports, tr!(mcp_error, reason = err), Tone::Fault);
            }
            begin(parts, entry, url, keys, reports);
        }
        // `/provider key` never comes here (T70): `on_secret_key` sends it to `key::change`.
        SecretFor::Key { .. } => {}
    }
}

/// What a sign-out did, in the owner's words.
fn signed_out(name: &str, done: &SignedOut) -> String {
    match done {
        SignedOut::Nothing => tr!(mcp_out_nothing, name = name),
        SignedOut::Token => tr!(mcp_out_token, name = name),
        SignedOut::Revoked => tr!(mcp_out_revoked, name = name),
        SignedOut::NotOffered => tr!(mcp_out_here, name = name),
        SignedOut::RevokeFailed(reason) => tr!(mcp_out_failed, name = name, reason = reason),
    }
}

/// `/mcp logout <name>`: revoked at the server where it offers that, forgotten here, its tools withdrawn.
pub fn logout(parts: Arc<Parts>, name: &str, reports: mpsc::UnboundedSender<Report>) {
    let found = entry(&parts, name).and_then(|entry| hosted(&entry));
    let (url, keys) = match found.and_then(|url| Ok((url, keys(&parts)?))) {
        Ok(found) => found,
        Err(reason) => return note(&reports, reason, Tone::Quiet),
    };
    let name = name.to_owned();
    tokio::spawn(async move {
        match oauth::sign_out(&name, &url, &keys).await {
            Ok(done) => {
                if done != SignedOut::Nothing {
                    parts.tools.servers().signed_out(&name);
                }
                note(&reports, signed_out(&name, &done), Tone::Quiet);
            }
            Err(err) => note(&reports, tr!(mcp_error, reason = err), Tone::Fault),
        }
    });
}

/// `/mcp add <name> <url>` or `/mcp add <name> -- <command…>`: into the owner's file, then connected at once.
pub fn add(parts: Arc<Parts>, args: &str, reports: mpsc::UnboundedSender<Report>) {
    let Some((name, what)) = args.trim().split_once(char::is_whitespace) else {
        return note(&reports, tr!(mcp_usage).to_owned(), Tone::Quiet);
    };
    let what = what.trim();
    let adding = match what.strip_prefix("--") {
        Some(command) => {
            let mut words = command.split_whitespace().map(str::to_owned);
            match words.next() {
                Some(command) => Adding::Stdio {
                    command,
                    args: words.collect(),
                },
                None => return note(&reports, tr!(mcp_usage).to_owned(), Tone::Quiet),
            }
        }
        None if !what.contains(char::is_whitespace) => Adding::Http(what.to_owned()),
        None => return note(&reports, tr!(mcp_usage).to_owned(), Tone::Quiet),
    };
    let entry = match config::add(&parts.home, name, adding) {
        Ok(entry) => entry,
        Err(err) => return note(&reports, tr!(mcp_error, reason = err), Tone::Fault),
    };
    note(&reports, tr!(mcp_added, name = name), Tone::Quiet);
    let keys = keys(&parts).unwrap_or_default();
    tokio::spawn(async move {
        let name = entry.name.clone();
        match parts.tools.servers().connect(entry, &keys).await {
            State::Connected { tools } => note(
                &reports,
                tr!(mcp_up, name = name, count = tools),
                Tone::Fine,
            ),
            State::SignIn => note(&reports, tr!(mcp_wants_note, name = name), Tone::Heed),
            State::Failed(reason) => note(
                &reports,
                tr!(mcp_not_up, name = name, reason = reason),
                Tone::Fault,
            ),
            State::Starting | State::Refused => {}
        }
    });
}

/// `/mcp remove <name>`: out of the owner's file — never a project's —, signed out, gone from the session.
pub fn remove(parts: Arc<Parts>, name: &str, reports: mpsc::UnboundedSender<Report>) {
    let owned = config::owner(&parts.home)
        .map_err(|err| tr!(mcp_error, reason = err))
        .and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.name == name)
                .ok_or_else(|| tr!(mcp_not_owners, name = name))
        });
    let entry = match owned {
        Ok(entry) => entry,
        Err(reason) => return note(&reports, reason, Tone::Quiet),
    };
    let name = name.to_owned();
    tokio::spawn(async move {
        if let ServerConfig::Http { url, .. } = &entry.config
            && let Ok(keys) = keys(&parts)
        {
            let _ = keys.forget_client_secret(&name, url);
            match oauth::sign_out(&name, url, &keys).await {
                Ok(SignedOut::Nothing) => {}
                Ok(done) => note(&reports, signed_out(&name, &done), Tone::Quiet),
                Err(err) => note(&reports, tr!(mcp_error, reason = err), Tone::Fault),
            }
        }
        match config::remove(&parts.home, &name) {
            Ok(_) => {
                parts.tools.servers().remove(&name);
                note(&reports, tr!(mcp_removed, name = name), Tone::Quiet);
            }
            Err(err) => note(&reports, tr!(mcp_error, reason = err), Tone::Fault),
        }
    });
}
