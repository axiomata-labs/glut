//! The MCP tools as the model sees them (`docs/plans/harness.md`, HS5, M5): one [`Tool`] per tool a connected
//! server offers, named `mcp__<server>__<tool>` and judged by the run's rules like every other tool.

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::model::ToolSpec;
use crate::permissions::Access;
use crate::tools::{CallContext, Tool, ToolFuture};

use super::client::{Client, RemoteTool};

/// The longest name the model API takes for a tool (`[a-zA-Z0-9_-]{1,64}`): a tool whose full name would run
/// past it is left out rather than sent, because the API refuses a request over one bad name — that would cost
/// the model every tool of this server instead of this one.
const MAX_MODEL_NAME: usize = 64;
/// How often a call that waits for its server checks whether the run was stopped (M3): a `tools/call` nobody
/// answers still ends with the run.
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// What a server's tool is called where the model sees it: `mcp__<server>__<tool>`, with every character the
/// model API does not take in a name (`.` and the like) replaced by `_`.
pub fn model_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The run's tools for one connected server: one per tool whose name fits, all sharing that server's client
/// (whose own lock serialises the calls, as a JSON-RPC line wants them).
pub fn offered(
    server: &str,
    tools: &[RemoteTool],
    client: Arc<Mutex<Client>>,
) -> Vec<Box<dyn Tool>> {
    let mut offered = Vec::new();
    for remote in tools {
        let name = model_name(server, &remote.name);
        if name.chars().count() > MAX_MODEL_NAME {
            // The name is a server's own word, so it is not printed raw to the owner's terminal.
            eprintln!(
                "mcp: the tool {} has no name the model API takes; left out",
                crate::permissions::printable(&format!("{}/{}", server, remote.name))
            );
            continue;
        }
        offered.push(Box::new(McpTool {
            server: server.to_owned(),
            tool: remote.name.clone(),
            name,
            description: remote
                .description
                .clone()
                .unwrap_or_else(|| format!("A tool of the MCP server {server}.")),
            schema: remote.input_schema.clone(),
            client: Arc::clone(&client),
        }) as Box<dyn Tool>);
    }
    offered
}

/// One tool of one connected server.
struct McpTool {
    /// The server's own name, as the rules spell it (`mcp__<server>…`).
    server: String,
    /// The tool's own name on that server, for `tools/call`.
    tool: String,
    /// [`model_name`]'s answer, what the model calls it.
    name: String,
    description: String,
    schema: Value,
    client: Arc<Mutex<Client>>,
}

impl Tool for McpTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.clone(),
            description: self.description.clone(),
            // An MCP tool may name no parameters at all, which still has to be an object for the model API.
            parameters: self.schema.clone(),
        }
    }

    fn access(&self, _arguments: &Value) -> Access {
        Access::Mcp {
            server: self.server.clone(),
            tool: self.tool.clone(),
        }
    }

    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_> {
        let client = Arc::clone(&self.client);
        let tool = self.tool.clone();
        Box::pin(async move {
            // The guard lives in this block so the call outlives the lock it took; the boxed future is what
            // gets dropped when a stop ends the wait.
            let mut guard = client.lock().await;
            let mut answer = Box::pin(guard.call(&tool, arguments));
            loop {
                tokio::select! {
                    result = &mut answer => {
                        return match result {
                            Ok(text) => Ok(text),
                            Err(err) => Err(err.to_string()),
                        };
                    }
                    _ = tokio::time::sleep(STOP_POLL) => {
                        // A stop ends the wait rather than leaving it to the server's own timeout (M3).
                        if context.stop.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err("the run was stopped".to_owned());
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_is_named_the_way_a_rule_names_it() {
        assert_eq!(model_name("mail", "list_emails"), "mcp__mail__list_emails");
        // A server's own punctuation does not reach the model API: `mcp__mail__list.emails` would be refused.
        assert_eq!(model_name("mail", "list.emails"), "mcp__mail__list_emails");
        assert_eq!(model_name("a b", "x/y"), "mcp__a_b__x_y");
        assert!(
            model_name("mail", "send")
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        );
    }

    #[test]
    fn a_tool_whose_name_would_run_past_the_api_is_left_out() {
        let client = Arc::new(Mutex::new(Client::for_test()));
        let long = "x".repeat(70);
        let tools = vec![
            RemoteTool::for_test("short"),
            RemoteTool::for_test(&long),
            RemoteTool::for_test("also_ok"),
        ];
        let made = offered("mail", &tools, Arc::clone(&client));
        let names: Vec<String> = made.iter().map(|tool| tool.spec().name).collect();
        assert_eq!(
            names,
            ["mcp__mail__short", "mcp__mail__also_ok"],
            "a 70-character tool name is skipped, the rest offered"
        );
        // A server name at its own limit still fits a short tool.
        let long_server = "y".repeat(32);
        assert_eq!(
            model_name(&long_server, &"z".repeat(25)).len(),
            64,
            "the longest name the API takes"
        );
        assert_eq!(
            offered(&long_server, &[RemoteTool::for_test("t")], client).len(),
            1
        );
    }

    #[tokio::test]
    async fn an_mcp_tool_says_what_it_would_touch_and_is_judged_by_its_rule() {
        use crate::permissions::{Gate, Mode, Nobody, RuleSet};

        let tool = McpTool {
            server: "mail".into(),
            tool: "send".into(),
            name: model_name("mail", "send"),
            description: "send".into(),
            schema: serde_json::json!({"type": "object"}),
            client: Arc::new(Mutex::new(Client::for_test())),
        };
        assert_eq!(
            tool.access(&serde_json::json!({})),
            Access::Mcp {
                server: "mail".into(),
                tool: "send".into()
            },
            "the server's own names, which the rules are written with"
        );
        assert_eq!(tool.spec().name, "mcp__mail__send");
        assert_eq!(
            tool.spec().parameters,
            serde_json::json!({"type": "object"})
        );
        assert!(
            !tool.spec().description.is_empty(),
            "a description the model can work with"
        );

        // Without a rule the mode decides (M5), and nobody to ask means refused, never run.
        let gate = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        assert!(
            gate.judge(&tool.name, &tool.access(&serde_json::json!({})))
                .await
                .is_err(),
            "a headless ask refuses"
        );
    }
}
