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

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::event::{Event, EventSink, McpStatus};
use crate::tools::Tool;
use config::ServerEntry;

/// Starts every server a run may use, in parallel (M3), and offers their tools to the model. Each start is
/// reported as an [`Event::McpServer`]; one that fails is named in its event and left out — the run goes on
/// without it, the way it goes on when a tool answers an error.
pub async fn start(entries: Vec<ServerEntry>, sink: &mut dyn EventSink) -> Vec<Box<dyn Tool>> {
    let mut pending = JoinSet::new();
    for entry in entries {
        pending.spawn(async move {
            let name = entry.name.clone();
            match client::Client::connect(&name, &entry.config).await {
                Ok(connected) => (name, Ok(connected)),
                Err(err) => (name, Err(err.to_string())),
            }
        });
    }

    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
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
                let offered = tools::offered(&name, &listed, shared);
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
