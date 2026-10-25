//! Where a run's MCP servers come from (`docs/plans/harness.md`, HS5, M1): the owner's `~/.glut/mcp.json` and
//! the project's `.mcp.json`, both read in one shape and merged, with the owner's winning on a name both hold.
//!
//! Both files are Claude Code's `{"mcpServers": {…}}`, so a repository's servers work as they are. What a server
//! may hold is checked here, before anything starts: an unknown field, a name that is no name, a transport the
//! harness does not speak or a URL that would leave this machine in clear text all stop the run, the way a
//! settings file that does not parse does (P4) — an MCP server starts a process and sends data out, so half of
//! a configuration is worse than none.
//!
//! A project's file is read whatever the folder's trust is: trust (P5) speaks about rules, and a project's
//! server has a second gate of its own — the owner's approval of that very server ([`super::approval`], M2).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::HarnessError;

/// The owner's MCP file, below the home folder.
const OWNER_FILE: &str = ".glut/mcp.json";
/// The project's MCP file, in Claude Code's spelling so a repository's own servers run as they are.
const PROJECT_FILE: &str = ".mcp.json";
/// An MCP file larger than this is no MCP file.
const MAX_CONFIG_BYTES: u64 = 256 * 1024;
/// The transport a server speaks when its entry says nothing (and when it says `"stdio"`).
const STDIO: &str = "stdio";
/// The other transport the harness speaks: Streamable HTTP. `sse` is deliberately not accepted (M3).
const HTTP: &str = "http";
/// The most characters a server name may hold, and the only characters it may use: it ends up in a rule
/// (`mcp__<server>__<tool>`) and in a tool name the model API accepts (`[a-zA-Z0-9_-]{1,64}`).
const MAX_NAME_CHARS: usize = 32;

/// Where a server came from (M1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `~/.glut/mcp.json`: the owner's own, started without any approval.
    Owner,
    /// `<project>/.mcp.json`: started only once the owner approved this very server (M2).
    Project,
}

/// One server a run may start, and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerEntry {
    pub name: String,
    pub origin: Origin,
    pub config: ServerConfig,
}

impl ServerEntry {
    /// The fingerprint an approval is keyed by (M2): SHA-256 over the config's own JSON, so a changed command,
    /// argument, env, header or URL is a different server to the approvals file — it asks again.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] if the config cannot be written down as JSON, which only a bug could cause.
    pub fn config_hash(&self) -> Result<String, HarnessError> {
        use sha2::{Digest, Sha256};
        let text = serde_json::to_string(&self.config).map_err(|err| HarnessError::Mcp {
            server: self.name.clone(),
            message: format!("its config cannot be written down: {err}"),
        })?;
        let digest = Sha256::digest(text.as_bytes());
        let mut fingerprint = String::with_capacity(digest.len() * 2);
        for byte in digest {
            fingerprint.push_str(&format!("{byte:02x}"));
        }
        Ok(fingerprint)
    }

    /// What the server is, for the approval question: its command line or its URL, then one line for every
    /// `env` variable or header as written (`${NAME}` unexpanded). The owner approves everything
    /// [`Self::config_hash`] covers, so everything it covers is shown — an `env` the question left out could load
    /// code into the server unseen.
    pub fn describe(&self) -> String {
        let quoted = |part: &str| {
            // A part with a space in it must still read as one part at the terminal.
            if part.chars().any(char::is_whitespace) {
                format!("{part:?}")
            } else {
                part.to_owned()
            }
        };
        let mut lines = Vec::new();
        match &self.config {
            ServerConfig::Stdio { command, args, env } => {
                lines.push(
                    std::iter::once(command.as_str())
                        .chain(args.iter().map(String::as_str))
                        .map(quoted)
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                lines.extend(
                    env.iter()
                        .map(|(key, value)| format!("env {key}={}", quoted(value))),
                );
            }
            ServerConfig::Http {
                url,
                headers,
                oauth,
            } => {
                lines.push(url.clone());
                if let Some(client) = oauth {
                    lines.push(format!("oauth client {}", client.client_id));
                }
                lines.extend(
                    headers
                        .iter()
                        .map(|(name, value)| format!("header {name}: {value}")),
                );
            }
        }
        lines.join("\n")
    }
}

/// One server, as the run starts it: one of the two transports (M1, M3).
///
/// Serde writes it back in the files' own shape (`{"type": "stdio", …}`), which is what
/// [`ServerEntry::config_hash`]
/// fingerprints.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerConfig {
    /// A child process speaking JSON-RPC one message per line on its stdin and stdout.
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    /// Streamable HTTP: one `POST` per message, answered as JSON or as a Server-Sent Events stream.
    Http {
        url: String,
        headers: BTreeMap<String, String>,
        /// The client registered for it beforehand, where its authorization server registers none by itself (T56).
        /// Left out of the JSON when there is none, so the approval of a server written before stays its own.
        #[serde(skip_serializing_if = "Option::is_none")]
        oauth: Option<OAuthClient>,
    },
}

/// An OAuth client the owner registered with a server's authorization server (T56, `"oauth": {"clientId": …}`,
/// Claude Code's shape): for a server whose authorization server takes no dynamic registration (GitHub's). Its
/// secret, where it has one, waits in the secret store, never in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OAuthClient {
    pub client_id: String,
    /// The port the redirect comes back to, where the client was registered with one; any free port otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<u16>,
}

impl ServerConfig {
    /// Checks one file's entry against the transport it names and turns it into a [`ServerConfig`].
    ///
    /// `at` is where the entry was read, for the error's text. A project's stdio server may not set a variable
    /// that makes a program load code ([`loads_code`]): the server runs outside the sandbox, and one approval of
    /// `npx some-server` must not also be an approval of `NODE_OPTIONS=--require ./x.js`.
    fn from_raw(raw: Raw, at: &str, name: &str, origin: Origin) -> Result<Self, String> {
        if origin == Origin::Project
            && let Some(key) = raw.env.keys().find(|key| loads_code(key))
        {
            return Err(format!(
                "{at}: the project server {name:?} may not set {key}: it decides which code a program loads; \
                 set it in the owner's {OWNER_FILE} instead"
            ));
        }
        match raw.kind.as_deref().unwrap_or(STDIO) {
            STDIO => {
                if raw.url.is_some() || !raw.headers.is_empty() || raw.oauth.is_some() {
                    return Err(format!(
                        "{at}: the server {name:?} is stdio but has a url or headers"
                    ));
                }
                let command = raw
                    .command
                    .filter(|command| !command.trim().is_empty())
                    .ok_or_else(|| format!("{at}: the stdio server {name:?} has no command"))?;
                Ok(ServerConfig::Stdio {
                    command,
                    args: raw.args,
                    env: raw.env,
                })
            }
            HTTP => {
                if raw
                    .oauth
                    .as_ref()
                    .is_some_and(|client| client.client_id.trim().is_empty())
                {
                    return Err(format!(
                        "{at}: the http server {name:?} names an oauth client without its clientId"
                    ));
                }
                if raw.command.is_some() || !raw.args.is_empty() || !raw.env.is_empty() {
                    return Err(format!(
                        "{at}: the http server {name:?} has a command, args or env"
                    ));
                }
                let url = raw
                    .url
                    .filter(|url| !url.trim().is_empty())
                    .ok_or_else(|| format!("{at}: the http server {name:?} has no url"))?;
                // Headers carry credentials, so the URL is held to the rule a key is held to (M3).
                if !crate::model::secure_enough(&url) {
                    return Err(format!(
                        "{at}: the server {name:?} may only be reached over https, or over http on this machine"
                    ));
                }
                Ok(ServerConfig::Http {
                    url,
                    headers: raw.headers,
                    oauth: raw.oauth,
                })
            }
            other => Err(format!(
                "{at}: the server {name:?} names the transport {other:?}; the harness speaks {STDIO:?} and {HTTP:?}"
            )),
        }
    }
}

/// One file as it is written: only `mcpServers`, and only what a server entry may hold (M1).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(rename = "mcpServers", default)]
    servers: BTreeMap<String, Raw>,
}

/// One entry as it is written, before the transport's own fields are told apart. Everything is optional here
/// so the refusal can say which server of which file is at fault, rather than a line number of an anonymous
/// JSON blob.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    oauth: Option<OAuthClient>,
}

/// Whether `name` is a server name (M1): `[a-zA-Z0-9_-]{1,32}`, short enough to head a tool name the model API
/// accepts — and without `__`, the separator of `mcp__<server>__<tool>`: a server `mail__send` would otherwise
/// be what the rule `mcp__mail__send` names.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.contains("__")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Variables that decide which code a program loads, where it fetches it from or whom it trusts while doing so —
/// for every program (`PATH`, the shells' start files, the certificate stores) or for one runtime or launcher.
/// A project's server may not set them (`ServerConfig::from_raw`). Defence in depth: the approval question shows
/// every variable anyway, but a repository's `.mcp.json` should not be able to slip one past a quick yes.
const CODE_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "SHELL",
    "ENV",
    "BASH_ENV",
    "SHELLOPTS",
    "BASHOPTS",
    "PROMPT_COMMAND",
    "IFS",
    "ZDOTDIR",
    "CLASSPATH",
    "VIRTUAL_ENV",
    "GOPATH",
    "GOROOT",
    "GOBIN",
    "GOFLAGS",
    "GOPROXY",
    "GOPRIVATE",
    "GONOSUMDB",
    "GONOSUMCHECK",
    "GOINSECURE",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "OPENSSL_CONF",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
];
/// Prefixes of such variables, family by family: the dynamic loaders, the shells' exported functions, the
/// runtimes and their package managers (each reads code paths, flags or a registry from its own variables), git,
/// and the folders programs read their configuration from.
const CODE_VAR_PREFIXES: &[&str] = &[
    "DYLD_",
    "LD_",
    "BASH_FUNC_",
    "NODE_",
    "NPM_CONFIG_",
    "YARN_",
    "PNPM_",
    "BUN_",
    "DENO_",
    "PYTHON",
    "PIP_",
    "UV_",
    "POETRY_",
    "CONDA",
    "RUBY",
    "GEM_",
    "BUNDLE_",
    "PERL",
    "JAVA_",
    "_JAVA_",
    "JDK_",
    "CARGO_",
    "RUSTC",
    "RUSTUP_",
    "DOTNET_",
    "GIT_",
    "XDG_",
];
/// Suffixes of such variables: a proxy decides what a launcher like `npx` or `uvx` actually downloads.
const CODE_VAR_SUFFIXES: &[&str] = &["_PROXY"];

/// Whether `key` is one of [`CODE_VARS`] or belongs to a family of [`CODE_VAR_PREFIXES`] or
/// [`CODE_VAR_SUFFIXES`] (case-blind: the file system and some runtimes do not care).
fn loads_code(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    CODE_VARS.contains(&key.as_str())
        || CODE_VAR_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
        || CODE_VAR_SUFFIXES.iter().any(|suffix| key.ends_with(suffix))
}

/// Both files read and merged (M1): the owner's first, then the project's — a name the owner already has wins,
/// so a repository cannot take over a server the owner configured.
///
/// # Errors
///
/// [`HarnessError::Settings`] for a file that exists but cannot be read or does not parse: an MCP file starts
/// processes, so a broken one stops the run instead of silently leaving servers out.
pub fn load(home: &Path, project: &Path) -> Result<Vec<ServerEntry>, HarnessError> {
    let mut entries = read(&home.join(OWNER_FILE), Origin::Owner)?;
    for entry in read(&project.join(PROJECT_FILE), Origin::Project)? {
        if !entries.iter().any(|known| known.name == entry.name) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// The owner's servers alone (`~/.glut/mcp.json`), checked as [`load`] checks them: the one list the app's own
/// Opencode instances start too (`docs/plans/agent-isolation.md`, D7).
///
/// # Errors
///
/// As [`load`], for the owner's file.
pub fn owner(home: &Path) -> Result<Vec<ServerEntry>, HarnessError> {
    read(&home.join(OWNER_FILE), Origin::Owner)
}

/// The servers of one file the caller names instead of the two places (`--mcp-config`, Claude Code's flag of the same
/// name): read and checked as [`load`] reads the owner's, and counted as the owner's — whoever names the file stands
/// for what it starts. Unlike the two places, a missing file is an error: it was named.
///
/// # Errors
///
/// As [`load`]; [`HarnessError::Settings`] when the file does not exist.
pub fn named(path: &Path) -> Result<Vec<ServerEntry>, HarnessError> {
    if !path.exists() {
        return Err(HarnessError::Settings {
            path: path.display().to_string(),
            reason: "no such file".into(),
        });
    }
    read(path, Origin::Owner)
}

/// One file's servers, in the order their names sort in (a `BTreeMap`), so a run starts them the same way every
/// time.
fn read(path: &Path, origin: Origin) -> Result<Vec<ServerEntry>, HarnessError> {
    let fail = |reason: String| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    };
    let Some(text) = read_text(path)? else {
        // No file says nothing: a project without servers, an owner who configured none.
        return Ok(Vec::new());
    };
    parse(&text, &path.display().to_string(), origin).map_err(fail)
}

/// One file's text, `None` when there is none.
fn read_text(path: &Path) -> Result<Option<String>, HarnessError> {
    let fail = |reason: String| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    };
    use std::io::Read as _;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(fail(err.to_string())),
    };
    // Opened once and checked on the open file: a FIFO or a file swapped in between is not read.
    if !file
        .metadata()
        .map_err(|err| fail(err.to_string()))?
        .is_file()
    {
        return Err(fail("not a regular file".into()));
    }
    let mut text = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| fail(err.to_string()))?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err(fail(format!("larger than {MAX_CONFIG_BYTES} bytes")));
    }
    Ok(Some(text))
}

/// A file's text checked and turned into its servers; `at` names it in a refusal.
fn parse(text: &str, at: &str, origin: Origin) -> Result<Vec<ServerEntry>, String> {
    let file: File = serde_json::from_str(text).map_err(|err| err.to_string())?;
    file.servers
        .into_iter()
        .map(|(name, raw)| {
            if !valid_name(&name) {
                return Err(format!(
                    "{name:?} is no server name: [a-zA-Z0-9_-], up to {MAX_NAME_CHARS} characters"
                ));
            }
            let config = ServerConfig::from_raw(raw, at, &name, origin)?;
            Ok(ServerEntry {
                name,
                origin,
                config,
            })
        })
        .collect()
}

/// A server to add to the owner's file (T56, `/mcp add`, `glut mcp add`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adding {
    /// Reached over Streamable HTTP at this URL.
    Http(String),
    /// Started as this command line.
    Stdio { command: String, args: Vec<String> },
}

/// Adds the server `name` to the owner's `~/.glut/mcp.json` — never a project's — and hands back its entry, checked
/// as a run checks it. The rest of the file stays as it was written (`${…}` and all); a name it holds already is
/// refused, not replaced.
///
/// # Errors
///
/// [`HarnessError::Settings`] for a name that is no name or is taken, a server the checks refuse (an http URL to
/// another machine), a file that cannot be read or is a link, or one that cannot be written.
pub fn add(home: &Path, name: &str, adding: Adding) -> Result<ServerEntry, HarnessError> {
    let path = home.join(OWNER_FILE);
    let fail = |reason: String| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    };
    if !valid_name(name) {
        return Err(fail(format!(
            "{name:?} is no server name: [a-zA-Z0-9_-], up to {MAX_NAME_CHARS} characters"
        )));
    }
    let mut file = owner_json(&path)?;
    let servers = servers_of(&mut file).map_err(fail)?;
    if servers.contains_key(name) {
        return Err(fail(format!(
            "it holds a server {name:?} already; remove it first"
        )));
    }
    let server = match adding {
        Adding::Http(url) => serde_json::json!({"type": HTTP, "url": url}),
        Adding::Stdio { command, args } => serde_json::json!({"command": command, "args": args}),
    };
    servers.insert(name.to_owned(), server);
    let entries = write_owner(&path, &file)?;
    entries
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| fail(format!("{name:?} was not written")))
}

/// Takes the server `name` out of the owner's `~/.glut/mcp.json`; `false`: it holds none of that name.
///
/// # Errors
///
/// As [`add`], for reading and writing the file.
pub fn remove(home: &Path, name: &str) -> Result<bool, HarnessError> {
    let path = home.join(OWNER_FILE);
    let mut file = owner_json(&path)?;
    let servers = servers_of(&mut file).map_err(|reason| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    })?;
    if servers.remove(name).is_none() {
        return Ok(false);
    }
    write_owner(&path, &file)?;
    Ok(true)
}

/// The owner's file as JSON, an empty one where there is none; a link is refused — the file it points to is the
/// owner's to edit, a rename over it would cut the link.
fn owner_json(path: &Path) -> Result<serde_json::Value, HarnessError> {
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(HarnessError::Settings {
            path: path.display().to_string(),
            reason: "it is a link; change the file it points to".into(),
        });
    }
    match read_text(path)? {
        Some(text) => serde_json::from_str(&text).map_err(|err| HarnessError::Settings {
            path: path.display().to_string(),
            reason: err.to_string(),
        }),
        None => Ok(serde_json::json!({"mcpServers": {}})),
    }
}

/// The `mcpServers` object of a file, made where it has none.
fn servers_of(
    file: &mut serde_json::Value,
) -> Result<&mut serde_json::Map<String, serde_json::Value>, String> {
    let object = file
        .as_object_mut()
        .ok_or_else(|| "it is no JSON object".to_owned())?;
    object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "its mcpServers is no object".to_owned())
}

/// Writes the owner's file whole once it passes the checks a run makes, readable by the owner alone (a header may
/// hold a token).
fn write_owner(path: &Path, file: &serde_json::Value) -> Result<Vec<ServerEntry>, HarnessError> {
    let fail = |reason: String| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    };
    let mut text = serde_json::to_string_pretty(file).map_err(|err| fail(err.to_string()))?;
    text.push('\n');
    let entries = parse(&text, &path.display().to_string(), Origin::Owner).map_err(fail)?;
    let dir = path.parent().ok_or_else(|| fail("no folder".into()))?;
    crate::providers::make_private_dir(dir).map_err(|err| fail(err.to_string()))?;
    crate::providers::write_replacing(dir, path, text.as_bytes(), 0o600)
        .map_err(|err| fail(err.to_string()))?;
    Ok(entries)
}

/// `${NAME}` in a header value, looked up through `lookup` (M3). No OAuth in HS5, so a header is where a token
/// is typed by hand, and a variable is what keeps it out of the file.
///
/// # Errors
///
/// A `${` with no closing `}`, or a name `lookup` does not hold — that server fails to start with this as its
/// reason; the run goes on without it.
pub fn expand_vars(text: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("${") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find('}') else {
            // The value itself is not quoted: it may hold a token typed beside the variable, and this reason
            // travels into events and a run's record.
            return Err("a ${ with no closing brace".to_owned());
        };
        let name = &after[..close];
        let value =
            lookup(name).ok_or_else(|| format!("the environment holds no {name:?} to put in"))?;
        out.push_str(&value);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glut-mcp-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_named_file_is_read_as_the_owners_and_must_exist() {
        let dir = temp("named");
        let file = dir.join("card-mcp.json");
        assert!(
            named(&file).is_err(),
            "a named file that is not there is an error"
        );
        write(
            &file,
            r#"{"mcpServers": {"axiomata": {"type": "stdio", "command": "/bin/cli", "args": ["mcp-serve"], "env": {"A": "1"}}}}"#,
        );
        let entries = named(&file).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "axiomata");
        assert_eq!(entries[0].origin, Origin::Owner);
        write(
            &file,
            r#"{"mcpServers": {"x": {"command": "a", "surprise": 1}}}"#,
        );
        assert!(named(&file).is_err(), "checked as the owner's file is");
    }

    #[test]
    fn both_files_are_merged_and_the_owner_wins() {
        let (home, project) = (temp("merge-home"), temp("merge-project"));
        write(
            &home.join(".glut/mcp.json"),
            r#"{"mcpServers": {"mail": {"command": "uvx", "args": ["mcp-mail"]}, "own": {"command": "a"}}}"#,
        );
        write(
            &project.join(".mcp.json"),
            r#"{"mcpServers": {"mail": {"command": "evil"}, "repo": {"type": "http", "url": "https://x.example"}}}"#,
        );
        let entries = load(&home, &project).unwrap();
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["mail", "own", "repo"]);
        let mail = &entries[0];
        assert_eq!(mail.origin, Origin::Owner);
        assert_eq!(
            mail.config,
            ServerConfig::Stdio {
                command: "uvx".into(),
                args: vec!["mcp-mail".into()],
                env: BTreeMap::new(),
            },
            "the project's server of that name does not take the owner's over"
        );
        assert_eq!(entries[2].origin, Origin::Project);
    }

    #[test]
    fn an_unknown_field_a_bad_name_or_a_bad_url_stops_the_run() {
        let dir = temp("strict");
        let broken = [
            // An unknown field: a misspelled `command` must not leave a server that starts nothing.
            r#"{"mcpServers": {"a": {"comand": "x"}}}"#,
            r#"{"mcpServers": {"a": {"type": "sse", "url": "https://x.example"}}}"#,
            r#"{"other": {}}"#,
            r#"{"mcpServers": {"Bad Name": {"command": "x"}}}"#,
            r#"{"mcpServers": {"": {"command": "x"}}}"#,
            r#"{"mcpServers": {"a": {"type": "http", "url": "http://elsewhere.example"}}}"#,
            r#"{"mcpServers": {"a": {"type": "http"}}}"#,
            r#"{"mcpServers": {"a": {"args": ["x"]}}}"#,
            r#"{"mcpServers": {"a": {"command": "x", "url": "https://x.example"}}}"#,
        ];
        for text in broken {
            write(&dir.join(".glut/mcp.json"), text);
            assert!(load(&dir, &dir).is_err(), "{text}");
        }
        // And what is fine: the default transport, a loopback http URL, a name at the length limit.
        write(
            &dir.join(".glut/mcp.json"),
            r#"{"mcpServers": {"a": {"type": "stdio", "command": "x"},
                               "b": {"type": "http", "url": "http://127.0.0.1:1/mcp"},
                               "abcdefghijklmnopqrstuvwxyz012345": {"command": "x"}}}"#,
        );
        let entries = load(&dir, &dir).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn a_file_that_does_not_parse_is_named_in_the_error() {
        let dir = temp("broken");
        write(&dir.join(".glut/mcp.json"), "{ not json");
        let Err(HarnessError::Settings { path, .. }) = load(&dir, &dir) else {
            panic!("a broken file must stop the run");
        };
        assert!(path.ends_with(".glut/mcp.json"), "{path}");
        // A missing file says nothing at all.
        let empty = temp("missing");
        assert!(load(&empty, &empty).unwrap().is_empty());
    }

    #[test]
    fn a_hash_covers_the_whole_config_and_changes_with_it() {
        let base = ServerEntry {
            name: "mail".into(),
            origin: Origin::Project,
            config: ServerConfig::Stdio {
                command: "uvx".into(),
                args: vec!["mcp-apple-mail".into()],
                env: BTreeMap::new(),
            },
        };
        let same = base.clone();
        assert_eq!(base.config_hash().unwrap(), same.config_hash().unwrap());
        let mut other_args = base.clone();
        other_args.config = ServerConfig::Stdio {
            command: "uvx".into(),
            args: vec!["mcp-apple-mail".into(), "--extra".into()],
            env: BTreeMap::new(),
        };
        assert_ne!(
            base.config_hash().unwrap(),
            other_args.config_hash().unwrap()
        );
        let mut other_url = base.clone();
        other_url.config = ServerConfig::Http {
            url: "https://x.example".into(),
            headers: BTreeMap::new(),
            oauth: None,
        };
        assert_ne!(
            base.config_hash().unwrap(),
            other_url.config_hash().unwrap()
        );
        // The order keys are written in is no difference: the canonical JSON sorts them.
        let mut sorted_env = base.clone();
        sorted_env.config = ServerConfig::Stdio {
            command: "uvx".into(),
            args: vec!["mcp-apple-mail".into()],
            env: BTreeMap::from([("B".into(), "2".into()), ("A".into(), "1".into())]),
        };
        let mut unsorted = sorted_env.clone();
        unsorted.config = ServerConfig::Stdio {
            command: "uvx".into(),
            args: vec!["mcp-apple-mail".into()],
            env: BTreeMap::from([("A".into(), "1".into()), ("B".into(), "2".into())]),
        };
        assert_eq!(
            sorted_env.config_hash().unwrap(),
            unsorted.config_hash().unwrap()
        );
    }

    #[test]
    fn a_description_line_is_the_command_line_or_the_url() {
        let stdio = ServerEntry {
            name: "mail".into(),
            origin: Origin::Owner,
            config: ServerConfig::Stdio {
                command: "uvx".into(),
                args: vec!["--with".into(), "mcp<2".into(), "mcp-apple-mail".into()],
                env: BTreeMap::new(),
            },
        };
        assert_eq!(stdio.describe(), "uvx --with mcp<2 mcp-apple-mail");
        let spaced = ServerEntry {
            config: ServerConfig::Stdio {
                command: "run".into(),
                args: vec!["two words".into()],
                env: BTreeMap::new(),
            },
            ..stdio.clone()
        };
        assert_eq!(spaced.describe(), r#"run "two words""#);
        let http = ServerEntry {
            config: ServerConfig::Http {
                url: "https://x.example/mcp".into(),
                headers: BTreeMap::new(),
                oauth: None,
            },
            ..stdio
        };
        assert_eq!(http.describe(), "https://x.example/mcp");

        // Everything the approval's hash covers is shown: `env` and headers as written, variables unexpanded.
        let with_env = ServerEntry {
            config: ServerConfig::Stdio {
                command: "npx".into(),
                args: vec!["srv".into()],
                env: BTreeMap::from([("TOKEN".into(), "a b".into())]),
            },
            ..http.clone()
        };
        assert_eq!(with_env.describe(), "npx srv\nenv TOKEN=\"a b\"");
        let with_header = ServerEntry {
            config: ServerConfig::Http {
                url: "https://x.example/mcp".into(),
                headers: BTreeMap::from([("Authorization".into(), "Bearer ${TOKEN}".into())]),
                oauth: None,
            },
            ..http
        };
        assert_eq!(
            with_header.describe(),
            "https://x.example/mcp\nheader Authorization: Bearer ${TOKEN}"
        );
    }

    #[test]
    fn a_project_server_may_not_set_what_loads_code_and_no_name_holds_the_separator() {
        let dir = temp("loader-vars");
        let home = dir.join("home");
        let project = dir.join("project");
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        for key in [
            "NODE_OPTIONS",
            "DYLD_INSERT_LIBRARIES",
            "path",
            "LD_PRELOAD",
        ] {
            std::fs::write(
                project.join(PROJECT_FILE),
                format!(
                    r#"{{"mcpServers": {{"fs": {{"command": "npx", "env": {{"{key}": "x"}}}}}}}}"#
                ),
            )
            .unwrap();
            let err = load(&home, &project).unwrap_err().to_string();
            assert!(err.contains("may not set"), "{key}: {err}");
        }
        // The owner's own file may: that is the owner's word, not a repository's.
        std::fs::remove_file(project.join(PROJECT_FILE)).unwrap();
        std::fs::write(
            home.join(OWNER_FILE),
            r#"{"mcpServers": {"fs": {"command": "npx", "env": {"NODE_OPTIONS": "--max-old-space-size=4096"}}}}"#,
        )
        .unwrap();
        assert_eq!(load(&home, &project).unwrap().len(), 1);
        // The families: launchers' registries, git's config, proxies, exported shell functions.
        for key in [
            "npm_config_registry",
            "UV_INDEX_URL",
            "GIT_CONFIG_GLOBAL",
            "https_proxy",
            "BASH_FUNC_ls%%",
            "PYTHONUSERBASE",
            "SSL_CERT_FILE",
        ] {
            assert!(loads_code(key), "{key}");
        }
        // Ordinary settings and tokens stay a project's to set.
        for key in [
            "GITHUB_TOKEN",
            "GOOGLE_API_KEY",
            "MAIL_ACCOUNT",
            "LOG_LEVEL",
        ] {
            assert!(!loads_code(key), "{key}");
        }

        assert!(
            !valid_name("mail__send"),
            "`__` would blur mcp__<server>__<tool>"
        );
        assert!(valid_name("my_mail-2"));
    }

    #[test]
    fn a_header_variable_is_put_in_and_a_missing_one_is_an_error() {
        let lookup = |name: &str| match name {
            "TOKEN" => Some("s3cret".to_owned()),
            _ => None,
        };
        assert_eq!(
            expand_vars("Bearer ${TOKEN}", lookup).unwrap(),
            "Bearer s3cret"
        );
        assert_eq!(expand_vars("no variable", lookup).unwrap(), "no variable");
        assert_eq!(
            expand_vars("${TOKEN}${TOKEN}", lookup).unwrap(),
            "s3crets3cret"
        );
        let missing = expand_vars("Bearer sk-typed ${NOPE}", lookup).unwrap_err();
        assert!(missing.contains("NOPE"), "{missing}");
        // The reason ends up in events and the run's record: the value around the variable stays out of it.
        assert!(!missing.contains("sk-typed"), "{missing}");
        let unclosed = expand_vars("Bearer sk-typed ${TOKEN", lookup).unwrap_err();
        assert!(!unclosed.contains("sk-typed"), "{unclosed}");
        assert!(
            expand_vars("Bearer ${TOKEN", lookup).is_err(),
            "an unclosed ${{ is refused, not passed on"
        );
        assert!(
            expand_vars("Bearer ${}", lookup).is_err(),
            "an empty name is refused"
        );
    }

    #[test]
    fn a_server_is_added_to_and_removed_from_the_owners_file_only_as_a_run_would_take_it() {
        let home = temp("add");
        write(
            &home.join(OWNER_FILE),
            r#"{"mcpServers": {"mail": {"command": "uvx", "env": {"T": "${TOKEN}"}}}}"#,
        );
        let notion = add(
            &home,
            "notion",
            Adding::Http("https://mcp.notion.com/mcp".into()),
        )
        .unwrap();
        assert!(
            matches!(notion.config, ServerConfig::Http { ref url, .. } if url.ends_with("/mcp"))
        );
        add(
            &home,
            "fs",
            Adding::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "server-fs".into()],
            },
        )
        .unwrap();
        let names: Vec<String> = owner(&home)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["fs", "mail", "notion"]);
        let text = std::fs::read_to_string(home.join(OWNER_FILE)).unwrap();
        assert!(
            text.contains("${TOKEN}"),
            "the rest stays as written: {text}"
        );
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(home.join(OWNER_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(
            add(&home, "mail", Adding::Http("https://x.example".into())).is_err(),
            "taken"
        );
        assert!(
            add(
                &home,
                "plain",
                Adding::Http("http://elsewhere.example/mcp".into())
            )
            .is_err(),
            "no clear text to another machine"
        );
        assert!(add(&home, "Bad Name", Adding::Http("https://x.example".into())).is_err());
        assert!(remove(&home, "notion").unwrap());
        assert!(!remove(&home, "notion").unwrap());
        assert_eq!(owner(&home).unwrap().len(), 2);
        let fresh = temp("add-fresh");
        add(&fresh, "a", Adding::Http("https://a.example/mcp".into())).unwrap();
        assert_eq!(
            owner(&fresh).unwrap().len(),
            1,
            "a file made where there was none"
        );
    }
}
