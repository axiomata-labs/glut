//! `glut mcp` (`docs/plans/glut-tui.md`, T56): the MCP servers of `~/.glut/mcp.json` and the folder's `.mcp.json`,
//! signing in to a hosted one and out again, and adding or removing the owner's own — the same as `/mcp` in the
//! terminal UI. A token is read without echo, never taken as an argument; a project's file is never written.

use std::io::{BufRead as _, IsTerminal as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;

use crate::mcp::config::{self, Adding, Origin, ServerConfig, ServerEntry};
use crate::mcp::keys::Signed;
use crate::mcp::oauth::{self, Begun, SignedOut};
use crate::mcp::{Client, Keys};

/// The longest token or secret read: far above any server's, a stop for a file piped in by mistake.
const MAX_SECRET_BYTES: u64 = 8 * 1024;

#[derive(Subcommand)]
pub enum McpCommand {
    /// The MCP servers of ~/.glut/mcp.json and the folder's .mcp.json, and what each one is signed in with (never
    /// the token).
    List {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
    /// Signs in to a hosted server: in the browser where it speaks OAuth, else with a token asked for without echo.
    Login {
        /// The server's name.
        name: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
    /// Signs out of a server: revoked at the server where it offers that, forgotten here either way.
    Logout {
        /// The server's name.
        name: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
    /// Adds a server to ~/.glut/mcp.json: its URL, or the command that starts it after --.
    Add {
        /// The server's name.
        name: String,
        /// A URL (https://…), or a command and its arguments after --.
        #[arg(required = true, num_args = 1.., trailing_var_arg = true, allow_hyphen_values = true)]
        what: Vec<String>,
    },
    /// Removes a server from ~/.glut/mcp.json and signs it out.
    Remove {
        /// The server's name.
        name: String,
    },
}

/// Runs one `glut mcp` command.
pub async fn run(command: McpCommand) -> ExitCode {
    let home = match crate::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    if !matches!(command, McpCommand::List { .. })
        && std::env::var_os(crate::tools::shell::SANDBOXED_ENV).is_some()
    {
        return fail(say!(owners_only));
    }
    let outcome = match command {
        McpCommand::List { cwd } => list(&home, &cwd),
        McpCommand::Login { name, cwd } => login(&home, &cwd, &name).await,
        McpCommand::Logout { name, cwd } => logout(&home, &cwd, &name).await,
        McpCommand::Add { name, what } => add(&home, &name, what).await,
        McpCommand::Remove { name } => remove(&home, &name).await,
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ExitCode {
    // A server's words may be in it: printable before they reach the terminal.
    eprintln!("glut mcp: {}", crate::permissions::printable(message));
    ExitCode::from(1)
}

/// The folder's servers, as a run in it reads them: the owner's first, then the project's.
fn servers(home: &Path, cwd: &Path) -> Result<Vec<ServerEntry>, String> {
    let cwd = std::fs::canonicalize(cwd)
        .map_err(|err| say!(cannot_use, path = cwd.display(), err = err))?;
    config::load(home, &cwd).map_err(|err| err.to_string())
}

fn keys(home: &Path) -> Result<Keys, String> {
    Keys::owner(home).map_err(|err| err.to_string())
}

/// The server `name` of the folder.
fn server(home: &Path, cwd: &Path, name: &str) -> Result<ServerEntry, String> {
    servers(home, cwd)?
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| say!(mcp_no_server, name = name))
}

/// What `entry` is, in a word of the list: its URL or its command line.
fn what_of(entry: &ServerEntry) -> String {
    let described = entry.describe();
    crate::permissions::printable(described.lines().next().unwrap_or_default())
}

fn list(home: &Path, cwd: &Path) -> Result<(), String> {
    let entries = servers(home, cwd)?;
    if entries.is_empty() {
        println!("{}", say!(mcp_none));
        return Ok(());
    }
    let keys = keys(home)?;
    for entry in &entries {
        let origin = match entry.origin {
            Origin::Owner => say!(mcp_origin_owner),
            Origin::Project => say!(mcp_origin_project),
        };
        let signed = match &entry.config {
            ServerConfig::Stdio { .. } => say!(mcp_kept_stdio),
            ServerConfig::Http { headers, .. }
                if headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("authorization")) =>
            {
                say!(mcp_kept_header)
            }
            ServerConfig::Http { url, .. } => {
                // One sign-in that cannot be read leaves the others listed.
                match keys.signed(&entry.name, url).ok().flatten() {
                    Some(Signed::OAuth(_)) => say!(mcp_kept_oauth),
                    Some(Signed::Token(_)) => say!(mcp_kept_token),
                    None => say!(mcp_kept_nothing),
                }
            }
        };
        println!("{}  {}  ({origin}) — {signed}", entry.name, what_of(entry));
    }
    Ok(())
}

/// The URL of a hosted server the owner can sign in to; a refusal says why not.
fn hosted(entry: &ServerEntry) -> Result<&str, String> {
    match &entry.config {
        ServerConfig::Stdio { .. } => Err(say!(mcp_stdio_no_login, name = entry.name)),
        ServerConfig::Http { headers, .. }
            if headers
                .keys()
                .any(|name| name.eq_ignore_ascii_case("authorization")) =>
        {
            Err(say!(mcp_header_login, name = entry.name))
        }
        ServerConfig::Http { url, .. } => Ok(url),
    }
}

async fn login(home: &Path, cwd: &Path, name: &str) -> Result<(), String> {
    let entry = server(home, cwd, name)?;
    // A project's server signs in only once the owner approved this very server — asked here, as `glut run` asks.
    if entry.origin == Origin::Project {
        let canonical = std::fs::canonicalize(cwd).map_err(|err| err.to_string())?;
        let permitted = crate::mcp::approval::permitted(
            vec![entry.clone()],
            home,
            &canonical,
            std::io::stdin().is_terminal(),
        )
        .map_err(|err| err.to_string())?;
        if permitted.is_empty() {
            return Err(say!(mcp_not_approved, name = name));
        }
    }
    let url = hosted(&entry)?.to_owned();
    let keys = keys(home)?;
    if let ServerConfig::Http {
        oauth: Some(client),
        ..
    } = &entry.config
        && keys
            .client_secret(name, &url)
            .map_err(|err| err.to_string())?
            .is_none()
        && std::io::stdin().is_terminal()
        && let Some(secret) = read_secret(&say!(mcp_secret_prompt, id = client.client_id), true)?
    {
        keys.keep_client_secret(name, &url, &secret)
            .map_err(|err| err.to_string())?;
    }
    match oauth::begin(name, &entry.config, &keys)
        .await
        .map_err(|err| err.to_string())?
    {
        Begun::Open => {
            println!("{}", say!(mcp_open, name = name));
            return Ok(());
        }
        Begun::Token { registered_only } => {
            if registered_only {
                println!("{}", say!(mcp_registered_only, name = name));
            }
            let host = crate::mcp::keys::host_of(&url);
            let Some(token) =
                read_secret(&say!(mcp_token_prompt, name = name, host = host), false)?
            else {
                return Err(say!(mcp_no_token).to_owned());
            };
            keys.keep_token(name, &url, &token)
                .map_err(|err| err.to_string())?;
        }
        Begun::SignIn(sign_in) => {
            println!(
                "{}",
                say!(mcp_sign_in_at, host = sign_in.host(), name = name)
            );
            println!("{}", sign_in.address());
            if let Some(host) = sign_in.token_host() {
                println!("{}", say!(mcp_token_elsewhere, name = name, host = host));
            }
            let local = oauth::can_open();
            println!(
                "{}",
                if local {
                    say!(mcp_enter_opens)
                } else {
                    say!(mcp_open_yourself)
                }
            );
            // Read on a thread of its own, never awaited at the end: a read still waiting on the terminal when the
            // browser came back must not hold the process. An empty line opens the browser (the owner's word), any
            // other is the address pasted back.
            let (send, received) = tokio::sync::oneshot::channel::<String>();
            if std::io::stdin().is_terminal() {
                let address = sign_in.address().to_owned();
                std::thread::spawn(move || {
                    let mut lines = std::io::stdin().lock().lines();
                    while let Some(Ok(line)) = lines.next() {
                        let line = line.trim();
                        if !line.is_empty() {
                            let _ =
                                send.send(line.chars().take(MAX_SECRET_BYTES as usize).collect());
                            return;
                        }
                        if local && oauth::open_in_browser(&address) {
                            println!("{}", say!(mcp_browser_opened));
                        }
                    }
                });
            }
            let pasted = async { received.await.ok() };
            let grant = sign_in
                .finish(pasted)
                .await
                .map_err(|err| err.to_string())?;
            keys.keep_grant(name, &url, &grant)
                .map_err(|err| err.to_string())?;
        }
    }
    // Signed in: the server is reached once, so the owner knows it works.
    match Client::connect_signed(name, &entry.config, &keys).await {
        Ok(client) => println!(
            "{}",
            say!(mcp_signed_in, name = name, count = client.tools().len())
        ),
        Err(err) => println!(
            "{}",
            say!(
                mcp_signed_in_failed,
                name = name,
                reason = crate::permissions::printable(&err.to_string())
            )
        ),
    }
    Ok(())
}

async fn logout(home: &Path, cwd: &Path, name: &str) -> Result<(), String> {
    let entry = server(home, cwd, name)?;
    let url = hosted(&entry)?.to_owned();
    let keys = keys(home)?;
    let done = oauth::sign_out(name, &url, &keys)
        .await
        .map_err(|err| err.to_string())?;
    println!("{}", signed_out(name, &done));
    Ok(())
}

/// What a sign-out did, in the owner's words.
fn signed_out(name: &str, done: &SignedOut) -> String {
    match done {
        SignedOut::Nothing => say!(mcp_nothing_kept, name = name),
        SignedOut::Token => say!(mcp_out_token, name = name),
        SignedOut::Revoked => say!(mcp_out_revoked, name = name),
        SignedOut::NotOffered => say!(mcp_out_here, name = name),
        SignedOut::RevokeFailed(reason) => say!(mcp_out_failed, name = name, reason = reason),
    }
}

async fn add(home: &Path, name: &str, what: Vec<String>) -> Result<(), String> {
    let adding = match what.as_slice() {
        [url] if url.starts_with("https://") || url.starts_with("http://") => {
            Adding::Http(url.clone())
        }
        [command, args @ ..] => Adding::Stdio {
            command: command.clone(),
            args: args.to_vec(),
        },
        [] => return Err(say!(mcp_add_what).to_owned()),
    };
    config::add(home, name, adding).map_err(|err| err.to_string())?;
    println!("{}", say!(mcp_added, name = name));
    Ok(())
}

async fn remove(home: &Path, name: &str) -> Result<(), String> {
    let entry = config::owner(home)
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| say!(mcp_not_owners, name = name))?;
    if let ServerConfig::Http { url, .. } = &entry.config {
        let keys = keys(home)?;
        keys.forget_client_secret(name, url)
            .map_err(|err| err.to_string())?;
        let done = oauth::sign_out(name, url, &keys)
            .await
            .map_err(|err| err.to_string())?;
        if done != SignedOut::Nothing {
            println!("{}", signed_out(name, &done));
        }
    }
    config::remove(home, name).map_err(|err| err.to_string())?;
    println!("{}", say!(mcp_removed, name = name));
    Ok(())
}

/// A token or secret from the terminal without echo, or from stdin when it is no terminal; `None` when nothing was
/// typed and `may_skip`.
fn read_secret(prompt: &str, may_skip: bool) -> Result<Option<String>, String> {
    let stdin = std::io::stdin();
    let text = if stdin.is_terminal() {
        eprint!("{prompt}");
        let _ = std::io::stderr().flush();
        let line = super::providers::without_echo(|| {
            let mut line = String::new();
            stdin
                .lock()
                .take(MAX_SECRET_BYTES + 1)
                .read_line(&mut line)
                .map(|_| line)
        });
        eprintln!();
        line.map_err(|err| say!(key_unreadable, err = err))?
    } else {
        let mut text = String::new();
        stdin
            .lock()
            .take(MAX_SECRET_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| say!(key_unreadable, err = err))?;
        text
    };
    if text.len() as u64 > MAX_SECRET_BYTES {
        return Err(say!(key_too_long, max = MAX_SECRET_BYTES));
    }
    let text = text.trim().to_owned();
    if text.is_empty() {
        return if may_skip {
            Ok(None)
        } else {
            Err(say!(mcp_no_token).to_owned())
        };
    }
    if text.chars().any(char::is_control) || text.contains(' ') {
        return Err(say!(key_odd).into());
    }
    Ok(Some(text))
}
