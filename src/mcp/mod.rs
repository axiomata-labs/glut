//! MCP servers (`docs/plans/harness.md`, HS5, M1–M5): the owner's `~/.axh/mcp.json` and the project's
//! `.mcp.json`, each project server approved on its own, two transports (stdio and Streamable HTTP), and the
//! tools they offer as `mcp__<server>__<tool>`.
//!
//! What holds a server is not a sandbox but the permissions on its tools (M5): a call is judged like any other,
//! by the rules (`mcp__server`, `mcp__server__tool`) and then by the mode. The owner's own servers are the
//! owner's word; a project's runs only once they said yes to that very config ([`approval`], M2).

pub mod approval;
pub mod client;
pub mod config;
pub mod tools;

pub use client::Client;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::event::{Event, EventSink, McpStatus};
use crate::tools::Tool;
use config::ServerEntry;

/// Starts every server a run may use, in parallel (M3), and offers their tools to the model. Each start is
/// reported as an [`Event::McpServer`]; one that fails is named in its event and left out — the run goes on
/// without it, the way it goes on when a tool answers an error.
///
/// Two servers whose names the model would see as one (`my-mail` and `my_mail` both head `mcp__my_mail__…`)
/// cannot both be offered — the API refuses a request naming a tool twice, and the model could not tell which
/// it calls. The first in `entries` (the owner's file comes before the project's) is kept, the other reported
/// as failed.
pub async fn start(entries: Vec<ServerEntry>, sink: &mut dyn EventSink) -> Vec<Box<dyn Tool>> {
    let mut pending = JoinSet::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    for entry in entries {
        let heading = tools::model_name(&entry.name, "");
        if let Some(first) = seen.get(&heading) {
            sink.emit(Event::McpServer {
                reason: Some(format!(
                    "its tools would be named like those of the server {first} ({heading}…); left out"
                )),
                name: entry.name,
                status: McpStatus::Failed,
                tools: 0,
            });
            continue;
        }
        seen.insert(heading, entry.name.clone());
        pending.spawn(async move {
            let name = entry.name.clone();
            match client::Client::connect(&name, &entry.config).await {
                Ok(connected) => (name, Ok(connected)),
                Err(err) => (name, Err(err.to_string())),
            }
        });
    }

    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    // Every model name offered so far: the API refuses a request that names a tool twice.
    let mut names: HashSet<String> = HashSet::new();
    while let Some(joined) = pending.join_next().await {
        let (name, result) = match joined {
            Ok(pair) => pair,
            // Our own code does not panic, but a panicking task must not take the other servers down with it.
            Err(err) => {
                sink.emit(Event::Error {
                    message: format!("an MCP server task ended unexpectedly: {err}"),
                });
                continue;
            }
        };
        match result {
            Ok(connected) => {
                let listed = connected.tools().to_vec();
                let shared = Arc::new(Mutex::new(connected));
                // Server names apart, two tools can still meet under one model name (`a.b` and `a_b`, or the
                // servers `a_` and `a` with the tools `x` and `_x`): the first one kept wins, like the servers.
                let offered: Vec<Box<dyn Tool>> = tools::offered(&name, &listed, shared)
                    .into_iter()
                    .filter(|tool| {
                        let fresh = names.insert(tool.spec().name);
                        if !fresh {
                            eprintln!(
                                "mcp: a tool of {} is named like one offered already; left out",
                                crate::permissions::printable(&name)
                            );
                        }
                        fresh
                    })
                    .collect();
                sink.emit(Event::McpServer {
                    name,
                    status: McpStatus::Connected,
                    tools: offered.len(),
                    reason: None,
                });
                tools.extend(offered);
            }
            Err(reason) => sink.emit(Event::McpServer {
                name,
                status: McpStatus::Failed,
                tools: 0,
                reason: Some(reason),
            }),
        }
    }
    tools
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
                command: "/nonexistent/axh-test-server".to_owned(),
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
        start(entries, &mut events).await;
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
