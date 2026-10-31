//! A run against a real model API. Ignored by default: it needs a key and costs a few tokens.
//!
//! `GLUT_LIVE_BASE_URL=https://openrouter.ai/api GLUT_LIVE_MODEL=deepseek/deepseek-chat GLUT_API_KEY=… \
//!  cargo test -p glut --test live -- --ignored --nocapture`

use glut::tools::files::{SkillRoots, file_tools};
use glut::{Cancel, Event, FinishReason, ModelEndpoint, OpenAiCompat, RunConfig, Toolbox, run};

#[tokio::test]
#[ignore = "needs a model API and a key"]
async fn a_real_model_reads_a_file_and_answers() {
    let base_url = std::env::var("GLUT_LIVE_BASE_URL").expect("GLUT_LIVE_BASE_URL");
    let model = std::env::var("GLUT_LIVE_MODEL").expect("GLUT_LIVE_MODEL");
    let dir = std::env::temp_dir().join(format!("glut-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("secret-word.txt"), "The word is: hexagon\n").unwrap();

    let model = OpenAiCompat::new(ModelEndpoint {
        base_url,
        api_key: std::env::var("GLUT_API_KEY").ok(),
        model,
        effort: None,
        versioned: false,
    })
    .unwrap();
    let mut tools = Toolbox::new();
    tools.extend(
        file_tools(
            &dir,
            SkillRoots::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap(),
    );
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

/// A real MCP server over stdio (HS5). Ignored by default: it runs whatever command `GLUT_LIVE_MCP` names, and a
/// test never names one of the owner's real app servers (`apple-mail`, `apple-reminders`) on its own — those are
/// reached from the app, not from a test.
///
/// `GLUT_LIVE_MCP="npx -y @modelcontextprotocol/server-everything" \
///  cargo test -p glut --test live -- --ignored --nocapture`
#[tokio::test]
#[ignore = "needs GLUT_LIVE_MCP"]
async fn a_real_stdio_server_connects_and_lists_its_tools() {
    use std::collections::BTreeMap;

    use glut::mcp::{Client, config::ServerConfig};

    let command = std::env::var("GLUT_LIVE_MCP").expect("GLUT_LIVE_MCP");
    let mut parts = command.split_whitespace();
    let config = ServerConfig::Stdio {
        command: parts
            .next()
            .expect("GLUT_LIVE_MCP names a command")
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

/// Keeping a cache warm against a real API (TUI T36e): a turn, then its first call sent again as a keep-warm — the
/// keep-warm must be taken (Anthropic: `max_tokens: 0`, unstreamed, with Glut's thinking and effort) and read the
/// cache. Ignored by default: it needs a key and costs a few cents of cache.
///
/// `GLUT_LIVE_BASE_URL=https://api.anthropic.com GLUT_LIVE_MODEL=claude-opus-5-5 GLUT_LIVE_PROTOCOL=anthropic \
///  GLUT_API_KEY=… cargo test -p glut --test live keeps_warm -- --ignored --nocapture`
#[tokio::test]
#[ignore = "needs a model API and a key"]
async fn a_real_model_keeps_its_cache_warm() {
    use glut::ChatModel as _;
    use glut::providers::catalog::Protocol;
    let base_url = std::env::var("GLUT_LIVE_BASE_URL").expect("GLUT_LIVE_BASE_URL");
    let model = std::env::var("GLUT_LIVE_MODEL").expect("GLUT_LIVE_MODEL");
    let protocol = match std::env::var("GLUT_LIVE_PROTOCOL").as_deref() {
        Ok("anthropic") => Protocol::AnthropicMessages,
        _ => Protocol::ChatCompletions,
    };
    let model = glut::Model::new(
        ModelEndpoint {
            base_url,
            api_key: std::env::var("GLUT_API_KEY").ok(),
            model,
            effort: None,
            versioned: false,
        },
        protocol,
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!("glut-live-warm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut config = RunConfig::new(&dir);
    // A head long enough for every provider's smallest cache.
    config.instructions = Some("Glut keeps this cache warm. ".repeat(600));
    let (outcome, conversation) = glut::run_continuing(
        &config,
        &model,
        &Toolbox::new(),
        Vec::new(),
        "Say hello in one word.",
        &mut Vec::new(),
        &Cancel::new(),
    )
    .await;
    assert_eq!(outcome.reason, FinishReason::Answered, "{outcome:?}");
    let warm = outcome.warm.expect("the run called the model");
    let usage = model
        .warm(&conversation[..warm.length], &warm.tools)
        .await
        .expect("the keep-warm is taken");
    println!("{usage:?}");
    assert!(
        usage.cached_input_tokens > 0,
        "it read the cache: {usage:?}"
    );
}
