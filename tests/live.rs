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
