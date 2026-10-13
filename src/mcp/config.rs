//! Where a run's MCP servers come from (`docs/plans/harness.md`, HS5, M1): the owner's `~/.axh/mcp.json` and
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
const OWNER_FILE: &str = ".axh/mcp.json";
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
    /// `~/.axh/mcp.json`: the owner's own, started without any approval.
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

    /// What the server is, on one line, for the approval question: its command line, or its URL.
    pub fn describe(&self) -> String {
        match &self.config {
            ServerConfig::Stdio { command, args, .. } => std::iter::once(command.as_str())
                .chain(args.iter().map(String::as_str))
                // An argument with a space in it must still read as one argument at the terminal.
                .map(|part| {
                    if part.chars().any(char::is_whitespace) {
                        format!("{part:?}")
                    } else {
                        part.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
            ServerConfig::Http { url, .. } => url.clone(),
        }
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
    },
}

impl ServerConfig {
    /// Checks one file's entry against the transport it names and turns it into a [`ServerConfig`].
    ///
    /// `at` is where the entry was read, for the error's text.
    fn from_raw(raw: Raw, at: &str, name: &str) -> Result<Self, String> {
        match raw.kind.as_deref().unwrap_or(STDIO) {
            STDIO => {
                if raw.url.is_some() || !raw.headers.is_empty() {
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
}

/// Whether `name` is a server name (M1): `[a-zA-Z0-9_-]{1,32}`, short enough to head a tool name the model API
/// accepts.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

/// One file's servers, in the order their names sort in (a `BTreeMap`), so a run starts them the same way every
/// time.
fn read(path: &Path, origin: Origin) -> Result<Vec<ServerEntry>, HarnessError> {
    let fail = |reason: String| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    };
    use std::io::Read as _;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        // No file says nothing: a project without servers, an owner who configured none.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
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
    let file: File = serde_json::from_str(&text).map_err(|err| fail(err.to_string()))?;
    let at = path.display().to_string();
    file.servers
        .into_iter()
        .map(|(name, raw)| {
            if !valid_name(&name) {
                return Err(fail(format!(
                    "{name:?} is no server name: [a-zA-Z0-9_-], up to {MAX_NAME_CHARS} characters"
                )));
            }
            let config = ServerConfig::from_raw(raw, &at, &name).map_err(fail)?;
            Ok(ServerEntry {
                name,
                origin,
                config,
            })
        })
        .collect()
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
            return Err(format!("{text:?} has a ${{ with no closing brace"));
        };
        let name = &after[..close];
        let value = lookup(name)
            .ok_or_else(|| format!("the environment holds no {name:?} to put into {text:?}"))?;
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
            "axh-mcp-{name}-{}-{}",
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
    fn both_files_are_merged_and_the_owner_wins() {
        let (home, project) = (temp("merge-home"), temp("merge-project"));
        write(
            &home.join(".axh/mcp.json"),
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
            write(&dir.join(".axh/mcp.json"), text);
            assert!(load(&dir, &dir).is_err(), "{text}");
        }
        // And what is fine: the default transport, a loopback http URL, a name at the length limit.
        write(
            &dir.join(".axh/mcp.json"),
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
        write(&dir.join(".axh/mcp.json"), "{ not json");
        let Err(HarnessError::Settings { path, .. }) = load(&dir, &dir) else {
            panic!("a broken file must stop the run");
        };
        assert!(path.ends_with(".axh/mcp.json"), "{path}");
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
            },
            ..stdio
        };
        assert_eq!(http.describe(), "https://x.example/mcp");
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
        let missing = expand_vars("Bearer ${NOPE}", lookup).unwrap_err();
        assert!(missing.contains("NOPE"), "{missing}");
        assert!(
            expand_vars("Bearer ${TOKEN", lookup).is_err(),
            "an unclosed ${{ is refused, not passed on"
        );
        assert!(
            expand_vars("Bearer ${}", lookup).is_err(),
            "an empty name is refused"
        );
    }
}
