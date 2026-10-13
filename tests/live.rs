//! A run against a real model API. Ignored by default: it needs a key and costs a few tokens.
//!
//! `AXH_LIVE_BASE_URL=https://openrouter.ai/api AXH_LIVE_MODEL=deepseek/deepseek-chat AXH_API_KEY=… \
//!  cargo test -p axiomata-harness --test live -- --ignored --nocapture`

use axiomata_harness::tools::files::file_tools;
use axiomata_harness::{
    Cancel, Event, FinishReason, ModelEndpoint, OpenAiCompat, RunConfig, Toolbox, run,
};

#[tokio::test]
#[ignore = "needs a model API and a key"]
async fn a_real_model_reads_a_file_and_answers() {
    let base_url = std::env::var("AXH_LIVE_BASE_URL").expect("AXH_LIVE_BASE_URL");
    let model = std::env::var("AXH_LIVE_MODEL").expect("AXH_LIVE_MODEL");
    let dir = std::env::temp_dir().join(format!("axh-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("secret-word.txt"), "The word is: hexagon\n").unwrap();

    let model = OpenAiCompat::new(ModelEndpoint {
        base_url,
        api_key: std::env::var("AXH_API_KEY").ok(),
        model,
    })
    .unwrap();
    let mut tools = Toolbox::new();
    tools.extend(file_tools(&dir).unwrap());
    let mut events = Vec::new();
    let outcome = run(
        &RunConfig::new(&dir),
        &model,
        &tools,
        "What is the word in the file secret-word.txt? Answer with the word only.",
        &mut events,
        &Cancel::new(),
    )
    .await;
    for event in &events {
        println!("{}", serde_json::to_string(event).unwrap());
    }
    assert_eq!(outcome.reason, FinishReason::Answered);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::ToolCall { name, .. } if name == "read_file"))
    );
    assert!(outcome.answer.unwrap().to_lowercase().contains("hexagon"));
}

/// A real MCP server over stdio (HS5). Ignored by default: it runs whatever command `AXH_LIVE_MCP` names, and a
/// test never names one of the owner's real app servers (`apple-mail`, `apple-reminders`) on its own — those are
/// reached from the app, not from a test.
///
/// `AXH_LIVE_MCP="npx -y @modelcontextprotocol/server-everything" \
///  cargo test -p axiomata-harness --test live -- --ignored --nocapture`
#[tokio::test]
#[ignore = "needs AXH_LIVE_MCP"]
async fn a_real_stdio_server_connects_and_lists_its_tools() {
    use std::collections::BTreeMap;

    use axiomata_harness::mcp::{Client, config::ServerConfig};

    let command = std::env::var("AXH_LIVE_MCP").expect("AXH_LIVE_MCP");
    let mut parts = command.split_whitespace();
    let config = ServerConfig::Stdio {
        command: parts
            .next()
            .expect("AXH_LIVE_MCP names a command")
            .to_owned(),
        args: parts.map(str::to_owned).collect(),
        env: BTreeMap::new(),
    };
    let client = Client::connect("live", &config)
        .await
        .expect("the server speaks the protocol");
    let names: Vec<&str> = client
        .tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    println!("{} tools: {names:?}", names.len());
    assert!(!names.is_empty(), "a real server offers at least one tool");
}
