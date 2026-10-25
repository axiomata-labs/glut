//! MCP servers (`docs/plans/harness.md`, HS5, M1–M5): the owner's `~/.glut/mcp.json` and the project's
//! `.mcp.json`, each project server approved on its own, two transports (stdio and Streamable HTTP), and the
//! tools they offer as `mcp__<server>__<tool>`.
//!
//! What holds a server is not a sandbox but the permissions on its tools (M5): a call is judged like any other,
//! by the rules (`mcp__server`, `mcp__server__tool`) and then by the mode. The owner's own servers are the
//! owner's word; a project's runs only once they said yes to that very config ([`approval`], M2).

pub mod approval;
pub mod client;
pub mod config;
pub mod keys;
pub mod oauth;
pub mod tools;

pub use client::Client;
pub use keys::Keys;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, PoisonError, RwLock};

use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::error::HarnessError;
use crate::event::{Event, EventSink, McpStatus};
use crate::tools::Tool;
use config::ServerEntry;

/// How a server of the session stands (T56): what `/mcp` and `glut mcp list` show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Being started.
    Starting,
    /// Handshake and tool list done: `tools` of its tools are offered.
    Connected { tools: usize },
    /// It wants a sign-in Glut does not hold, or one it no longer takes: `/mcp login`.
    SignIn,
    /// It could not be started or did not speak the protocol.
    Failed(String),
    /// A project's server the owner has not approved (M2): not started.
    Refused,
}

/// A server of the session, as [`Servers::list`] gives it.
#[derive(Debug, Clone)]
pub struct Listed {
    pub entry: ServerEntry,
    pub state: State,
}

/// The MCP servers of a session and the tools they offer (T56): shared by the session's toolbox, so a server signed
/// in later joins at once and one signed out or removed leaves — every turn and subagent that holds the box sees
/// it. Ordered by the servers' names, so the tools reach the model in the same order every time.
#[derive(Clone, Default)]
pub struct Servers(Arc<RwLock<BTreeMap<String, Joined>>>);

struct Joined {
    entry: ServerEntry,
    state: State,
    tools: Vec<Arc<dyn Tool>>,
}

impl Servers {
    /// Every server of the session, by name.
    pub fn list(&self) -> Vec<Listed> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|joined| Listed {
                entry: joined.entry.clone(),
                state: joined.state.clone(),
            })
            .collect()
    }

    /// The server `name`'s entry.
    pub fn entry(&self, name: &str) -> Option<ServerEntry> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .map(|joined| joined.entry.clone())
    }

    /// The tools of every connected server.
    pub(crate) fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .flat_map(|joined| joined.tools.iter().cloned())
            .collect()
    }

    fn set(&self, entry: ServerEntry, state: State, tools: Vec<Arc<dyn Tool>>) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                entry.name.clone(),
                Joined {
                    entry,
                    state,
                    tools,
                },
            );
    }

    /// Connects `entry`'s server with what `keys` holds for it — anew where it was connected, its old process and
    /// tools gone first — and says how it stands. A tool named like one of another server is left out: the API
    /// refuses a request that names a tool twice. The names are checked and the tools put in under one lock, so two
    /// servers that come up at once cannot both offer a name.
    pub async fn connect(&self, entry: ServerEntry, keys: &Keys) -> State {
        self.set(entry.clone(), State::Starting, Vec::new());
        let state = match Client::connect_signed(&entry.name, &entry.config, keys).await {
            Ok(connected) => {
                let listed = connected.tools().to_vec();
                let shared = Arc::new(Mutex::new(connected));
                let offered = tools::offered(&entry.name, &listed, shared);
                let mut servers = self.0.write().unwrap_or_else(PoisonError::into_inner);
                let mut taken: Vec<String> = servers
                    .iter()
                    .filter(|(name, _)| **name != entry.name)
                    .flat_map(|(_, joined)| joined.tools.iter().map(|tool| tool.spec().name))
                    .collect();
                let kept: Vec<Arc<dyn Tool>> = offered
                    .into_iter()
                    .filter(|tool| {
                        let name = tool.spec().name;
                        let fresh = !taken.contains(&name);
                        taken.push(name);
                        fresh
                    })
                    .map(Arc::from)
                    .collect();
                let state = State::Connected { tools: kept.len() };
                servers.insert(
                    entry.name.clone(),
                    Joined {
                        entry,
                        state: state.clone(),
                        tools: kept,
                    },
                );
                return state;
            }
            Err(HarnessError::McpSignIn { .. }) => State::SignIn,
            Err(err) => State::Failed(err.to_string()),
        };
        self.set(entry, state.clone(), Vec::new());
        state
    }

    /// A project's server the owner has not approved, listed as such: `/mcp` names it, nothing starts it.
    pub fn refused(&self, entry: ServerEntry) {
        self.set(entry, State::Refused, Vec::new());
    }

    /// The server `name`'s tools withdrawn and its process ended, the server kept as one that wants a sign-in (a
    /// sign-out).
    pub fn signed_out(&self, name: &str) {
        if let Some(joined) = self
            .0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(name)
        {
            joined.tools.clear();
            joined.state = State::SignIn;
        }
    }

    /// The server `name` gone from the session, its tools with it.
    pub fn remove(&self, name: &str) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
    }
}

/// What a server that wants a sign-in is reported with: no run signs in by itself.
pub fn sign_in_reason(name: &str) -> String {
    format!(
        "it wants a sign-in: glut mcp login {} (in Glut: /mcp login {0})",
        crate::permissions::printable(name)
    )
}

/// Starts every server a run may use, in parallel (M3), into `servers` — whose tools the run's toolbox offers —
/// with what `keys` holds for them (T56). Each start is reported as an [`Event::McpServer`]; one that fails or wants
/// a sign-in is named in its event and left out — the run goes on without it, the way it goes on when a tool
/// answers an error.
///
/// Two servers whose names the model would see as one (`my-mail` and `my_mail` both head `mcp__my_mail__…`)
/// cannot both be offered — the API refuses a request naming a tool twice, and the model could not tell which
/// it calls. The first in `entries` (the owner's file comes before the project's) is kept, the other reported
/// as failed.
pub async fn start(
    entries: Vec<ServerEntry>,
    keys: &Keys,
    servers: &Servers,
    sink: &mut dyn EventSink,
) {
    let mut pending = JoinSet::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    for entry in entries {
        let heading = tools::model_name(&entry.name, "");
        if let Some(first) = seen.get(&heading) {
            let reason = format!(
                "its tools would be named like those of the server {first} ({heading}…); left out"
            );
            sink.emit(Event::McpServer {
                reason: Some(reason),
                name: entry.name,
                status: McpStatus::Failed,
                tools: 0,
            });
            continue;
        }
        seen.insert(heading, entry.name.clone());
        let (servers, keys) = (servers.clone(), keys.clone());
        pending.spawn(async move {
            let name = entry.name.clone();
            (name, servers.connect(entry, &keys).await)
        });
    }
    while let Some(joined) = pending.join_next().await {
        let (name, state) = match joined {
            Ok(pair) => pair,
            // Our own code does not panic, but a panicking task must not take the other servers down with it.
            Err(err) => {
                sink.emit(Event::Error {
                    message: format!("an MCP server task ended unexpectedly: {err}"),
                });
                continue;
            }
        };
        let (status, tools, reason) = match state {
            State::Connected { tools } => (McpStatus::Connected, tools, None),
            State::SignIn => (McpStatus::Failed, 0, Some(sign_in_reason(&name))),
            State::Failed(reason) => (McpStatus::Failed, 0, Some(reason)),
            State::Starting | State::Refused => continue,
        };
        sink.emit(Event::McpServer {
            name,
            status,
            tools,
            reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use config::{Origin, ServerConfig};

    fn entry(name: &str, origin: Origin) -> ServerEntry {
        ServerEntry {
            name: name.to_owned(),
            origin,
            config: ServerConfig::Stdio {
                command: "/nonexistent/glut-test-server".to_owned(),
                args: Vec::new(),
                env: BTreeMap::new(),
            },
        }
    }

    #[tokio::test]
    async fn a_server_named_like_an_earlier_one_for_the_model_is_left_out() {
        let mut events: Vec<Event> = Vec::new();
        let entries = vec![
            entry("my-mail", Origin::Owner),
            entry("my_mail", Origin::Project),
        ];
        start(entries, &Keys::none(), &Servers::default(), &mut events).await;
        let reasons: BTreeMap<String, String> = events
            .into_iter()
            .filter_map(|event| match event {
                Event::McpServer { name, reason, .. } => Some((name, reason.unwrap_or_default())),
                _ => None,
            })
            .collect();
        assert!(
            reasons["my_mail"].contains("the server my-mail"),
            "{reasons:?}"
        );
        // The first one was started (and failed for its own reason, the missing program).
        assert!(!reasons["my-mail"].contains("named like"), "{reasons:?}");
    }
}
