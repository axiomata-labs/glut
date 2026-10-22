//! What each provider offers (`docs/plans/glut-providers.md`, PV2, P5): its model list, pulled from the provider
//! (`GET {root}/models`) and kept in `~/.axh/models/<id>.json` with its fetch time, so a run, the meter and the model
//! picker know a model's context window, its prices and whether it reasons without asking every time. The owner's
//! `prices` and `contextWindow` in `settings.json` still beat what a list says.

use super::catalog::Protocol;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ModelEndpoint, Prices};
use crate::error::HarnessError;

/// The folder of the lists, below `~/.axh`.
pub const DIR: &str = "models";
/// A list older than this is fetched again before a run uses it.
pub const FRESH_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a provider has to send its list.
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
/// The largest list read: OpenRouter's, with every model's prices and description, is about one MB.
const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;
/// After a failed fetch the kept list is used this long before the provider is asked again, so a provider without a
/// list (or one that is down) does not hold up every run.
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);
/// A price per million tokens above this is no price but a provider's mistake (or a lie to a budget).
const MAX_PRICE_PER_M: f64 = 10_000.0;
/// What a kept list holds: a list of an older format (one kept before the account's allowed models were asked) is
/// fetched again however young it is.
pub const FORMAT: u32 = 2;
/// The models.dev catalog: OpenCode's own description of every provider's models, where a gateway's list (ids
/// alone) finds each model's API, window and prices.
const DESCRIPTIONS_URL: &str = "https://models.dev/api.json";
/// The largest catalog read: about six MB in October 2026, with room to grow.
const MAX_DESCRIPTIONS_BYTES: usize = 16 * 1024 * 1024;
/// The longest display name taken from models.dev.
const MAX_NAME_CHARS: usize = 80;
/// The largest window taken from models.dev: beyond it a description is a mistake, and Glut would never compact.
const MAX_DESCRIBED_WINDOW: u32 = 10_000_000;

/// The API a model of a gateway speaks ([`super::catalog::Protocol::Gateway`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Api {
    /// OpenAI's chat completions.
    Chat,
    /// Anthropic's Messages API.
    Anthropic,
    /// OpenAI's Responses API: Glut does not speak it yet (PV4).
    Responses,
    /// Google's own API.
    Google,
    /// Anything else (a model that is no chat model).
    Other,
}

impl Api {
    /// Whether Glut has a client for it.
    pub fn spoken(self) -> bool {
        matches!(self, Api::Chat | Api::Anthropic)
    }

    /// What a gateway model speaks when models.dev does not describe it: OpenCode's endpoint table of 2026-10-22
    /// (Claude and Alibaba's Qwen 3.x through Anthropic's API, GPT, Grok and Muse through the Responses API, Gemini
    /// through Google's, the rest chat).
    pub fn guessed(model: &str) -> Api {
        let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| model.starts_with(prefix));
        if starts(&["claude-", "qwen3."]) {
            Api::Anthropic
        } else if starts(&["gpt-", "grok-", "muse-"]) {
            Api::Responses
        } else if starts(&["gemini-"]) {
            Api::Google
        } else if starts(&["jev-"]) {
            Api::Other
        } else {
            Api::Chat
        }
    }

    /// The package models.dev names a model's client by (`provider.npm`); none is the provider's own, chat.
    fn of_package(package: Option<&str>) -> Api {
        match package {
            None | Some("@ai-sdk/openai-compatible") => Api::Chat,
            Some("@ai-sdk/anthropic") => Api::Anthropic,
            Some("@ai-sdk/openai") => Api::Responses,
            Some("@ai-sdk/google") => Api::Google,
            Some(_) => Api::Other,
        }
    }
}

/// One model as its provider's list describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// The id a run names (`deepseek/deepseek-v4.1-flash`, `qwen3:8b`).
    pub id: String,
    /// A display name, where the list has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// USD per million tokens, where the list says (OpenRouter's does).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<Prices>,
    /// Whether the model reasons before it answers (`supported_parameters` naming `reasoning`); `None`: unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// When the provider added the model (Unix seconds, OpenRouter's `created`): the picker's "newest".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<i64>,
    /// The API the model speaks, for a gateway's model; `None` for every other provider's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<Api>,
}

impl ModelInfo {
    /// Whether the model costs nothing: a price of 0 both ways, or OpenRouter's `:free` variant.
    pub fn free(&self) -> bool {
        self.id.ends_with(":free")
            || self
                .prices
                .is_some_and(|prices| prices.input_per_m == 0.0 && prices.output_per_m == 0.0)
    }
}

/// A provider's list as kept on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelList {
    /// [`FORMAT`] when the list was kept; missing in the first format.
    #[serde(default)]
    pub format: u32,
    pub fetched_at: DateTime<Utc>,
    /// When a fetch last failed while this list was kept: it stands in for an hour ([`RETRY_AFTER`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<DateTime<Utc>>,
    /// The root the list came from: a list of another root (the provider was moved) is not this provider's.
    pub root: String,
    pub models: Vec<ModelInfo>,
    /// The ids the account may use, where the provider says (OpenRouter's `/models/user`: after its guardrails,
    /// privacy and provider settings); `None`: every model of the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed: Option<Vec<String>>,
}

impl ModelList {
    /// The model `id`.
    pub fn get(&self, id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|model| model.id == id)
    }

    /// Whether the account may use model `id`.
    pub fn allows(&self, id: &str) -> bool {
        self.allowed
            .as_ref()
            .is_none_or(|allowed| allowed.iter().any(|allowed| allowed == id))
    }

    /// Whether the list may be used without asking the provider: of the current [`FORMAT`] and younger than
    /// [`FRESH_FOR`], or a fetch failed less than [`RETRY_AFTER`] ago. A time in the future (a clock set back) is not
    /// fresh.
    pub fn fresh(&self, now: DateTime<Utc>) -> bool {
        let younger = |at: DateTime<Utc>, limit: Duration| {
            now.signed_duration_since(at)
                .to_std()
                .is_ok_and(|age| age < limit)
        };
        self.format == FORMAT && younger(self.fetched_at, FRESH_FOR)
            || self.failed_at.is_some_and(|at| younger(at, RETRY_AFTER))
    }

    /// The models whose id or name holds every word of `query`, in any case.
    pub fn matching<'a>(&'a self, query: &'a str) -> impl Iterator<Item = &'a ModelInfo> + 'a {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.models.iter().filter(move |model| {
            let text = format!(
                "{} {}",
                model.id.to_lowercase(),
                model.name.as_deref().unwrap_or("").to_lowercase()
            );
            words.iter().all(|word| text.contains(word.as_str()))
        })
    }
}

/// Where provider `id`'s list is kept.
pub fn path(home: &Path, id: &str) -> PathBuf {
    home.join(super::DIR).join(DIR).join(format!("{id}.json"))
}

/// The kept list of provider `id`, if there is one of its current `root`.
pub fn cached(home: &Path, id: &str, root: &str) -> Option<ModelList> {
    let mut text = String::new();
    super::open_no_follow(&path(home, id))
        .ok()?
        .take(MAX_LIST_BYTES as u64)
        .read_to_string(&mut text)
        .ok()?;
    let list: ModelList = serde_json::from_str(&text).ok()?;
    (list.root == root).then_some(list)
}

/// Keeps `list` as provider `id`'s.
///
/// # Errors
///
/// [`HarnessError::Settings`] when it cannot be written.
pub fn keep(home: &Path, id: &str, list: &ModelList) -> Result<(), HarnessError> {
    let file = path(home, id);
    let fail = |reason: String| HarnessError::Settings {
        path: file.display().to_string(),
        reason,
    };
    let dir = home.join(super::DIR).join(DIR);
    super::make_private_dir(&dir).map_err(|err| fail(err.to_string()))?;
    let text = serde_json::to_string(list).map_err(|err| fail(err.to_string()))?;
    super::write_replacing(&dir, &file, text.as_bytes(), 0o644).map_err(|err| fail(err.to_string()))
}

/// Pulls the list from the provider `endpoint` names (its root and key; the model is not used).
///
/// # Errors
///
/// A message when the provider cannot be reached, refuses, or answers no list.
pub async fn fetch(endpoint: &ModelEndpoint, protocol: Protocol) -> Result<ModelList, String> {
    let root = endpoint.base_url.trim_end_matches('/');
    if endpoint.api_key.is_some() && !crate::model::secure_enough(root) {
        return Err("a key is only sent over https, or over http to this machine".into());
    }
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        // As for a model call: an API has no reason to redirect, and following one would carry the key elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| err.to_string())?;
    let base = if endpoint.versioned {
        root.to_owned()
    } else {
        format!("{root}/v1")
    };
    if protocol == Protocol::Gateway {
        // A gateway's description is the catalog's, found by its root: a root of the owner's own (a proxy) has none.
        let described_by = super::catalog::CATALOG
            .iter()
            .find(|known| known.root == root)
            .and_then(|known| known.described_by);
        return gateway_list(&client, root, endpoint, described_by).await;
    }
    if protocol == Protocol::AnthropicMessages {
        // One page of a thousand is every model Anthropic has, many times over.
        let value = get_json(
            &client,
            &format!("{base}/models?limit=1000"),
            endpoint,
            protocol,
        )
        .await?;
        let mut models =
            crate::anthropic::models_of(&value).ok_or("no model list: no `data` array")?;
        models.sort_by(|a, b| a.id.cmp(&b.id));
        return Ok(ModelList {
            format: FORMAT,
            fetched_at: Utc::now(),
            failed_at: None,
            root: root.to_owned(),
            models,
            allowed: None,
        });
    }
    let value = get_json(&client, &format!("{base}/models"), endpoint, protocol).await?;
    let models = parse(&value).ok_or("no model list: no `data` array")?;
    // OpenRouter says which of them this account may use; a provider without such a list allows them all, and a
    // failed ask leaves the list as it is.
    let allowed = if crate::model::is_openrouter(root) && endpoint.api_key.is_some() {
        get_json(&client, &format!("{base}/models/user"), endpoint, protocol)
            .await
            .ok()
            .and_then(|value| parse(&value))
            .map(|models| models.into_iter().map(|model| model.id).collect::<Vec<_>>())
            // An empty answer says nothing about the account: every model stays usable.
            .filter(|allowed| !allowed.is_empty())
    } else {
        None
    };
    Ok(ModelList {
        format: FORMAT,
        fetched_at: Utc::now(),
        failed_at: None,
        root: root.to_owned(),
        models,
        allowed,
    })
}

/// A gateway's list: the ids from the gateway (`{root}/v1/models`, with the key), what each speaks and costs from
/// models.dev. Without the description every model's API is guessed from its id and its window and prices are
/// unknown; a model Glut cannot speak to stays in the list, not allowed, so the picker shows it greyed.
async fn gateway_list(
    client: &reqwest::Client,
    root: &str,
    endpoint: &ModelEndpoint,
    described_by: Option<&str>,
) -> Result<ModelList, String> {
    let value = get_json(
        client,
        &format!("{root}/v1/models"),
        endpoint,
        Protocol::Gateway,
    )
    .await?;
    let mut models = parse(&value).ok_or("no model list: no `data` array")?;
    let described = match described_by {
        Some(id) => descriptions(client, id).await.ok(),
        None => None,
    };
    let undescribed = described_by.is_some() && described.is_none();
    for model in &mut models {
        let found = described
            .as_ref()
            .and_then(|all| all.get(model.id.as_str()))
            .and_then(|entry| described_model(&model.id, entry));
        match found {
            Some(info) => {
                // The gateway's own `created` is the time of its list, the same for every model: the release
                // date says which is newest.
                *model = info;
            }
            None => {
                model.api = Some(Api::guessed(&model.id));
                // The gateway's `created` is its list's time, the same for every model: no "newest" in it.
                model.created = None;
            }
        }
    }
    let allowed = models
        .iter()
        .filter(|model| model.api.is_some_and(Api::spoken))
        .map(|model| model.id.clone())
        .collect();
    // A list without its description (models.dev did not answer) has guesses and no prices: it is asked for again
    // an hour later, not a day.
    let fetched_at = if undescribed {
        Utc::now() - (FRESH_FOR - RETRY_AFTER)
    } else {
        Utc::now()
    };
    Ok(ModelList {
        format: FORMAT,
        fetched_at,
        failed_at: None,
        root: root.to_owned(),
        models,
        allowed: Some(allowed),
    })
}

/// The models models.dev describes for its provider `id` (`{"<id>": {"models": {…}}}`), asked without a key.
async fn descriptions(
    client: &reqwest::Client,
    id: &str,
) -> Result<serde_json::Map<String, Value>, String> {
    let keyless = ModelEndpoint {
        base_url: String::new(),
        api_key: None,
        model: String::new(),
        effort: None,
        versioned: true,
    };
    let value = get_capped(
        client,
        DESCRIPTIONS_URL,
        &keyless,
        Protocol::ChatCompletions,
        MAX_DESCRIPTIONS_BYTES,
    )
    .await?;
    // Taken out of the catalog, not copied: it is the one part of six MB that is kept.
    let Value::Object(mut providers) = value else {
        return Err("models.dev answered no catalog".into());
    };
    match providers
        .remove(id)
        .and_then(|mut provider| provider.get_mut("models").map(Value::take))
    {
        Some(Value::Object(models)) => Ok(models),
        _ => Err(format!("models.dev describes no provider {id}")),
    }
}

/// One model as models.dev describes it: name, window, prices per million tokens, reasoning, release date and the
/// package of its client (which API it speaks).
fn described_model(id: &str, entry: &Value) -> Option<ModelInfo> {
    let entry = entry.as_object()?;
    let per_m = |key: &str| -> Option<f64> {
        let price = entry.get("cost")?.get(key)?.as_f64()?;
        (price.is_finite() && (0.0..=MAX_PRICE_PER_M).contains(&price)).then_some(price)
    };
    let prices = match (per_m("input"), per_m("output")) {
        // Nothing both ways is a price only for a model the gateway calls free: else a description's gap, which
        // would let a budget count nothing.
        (Some(input), Some(output)) if input == 0.0 && output == 0.0 && !id.contains("free") => {
            None
        }
        (Some(input_per_m), Some(output_per_m)) => Some(Prices {
            input_per_m,
            cached_input_per_m: per_m("cache_read"),
            output_per_m,
        }),
        _ => None,
    };
    let created = entry
        .get("release_date")
        .and_then(Value::as_str)
        .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|time| time.and_utc().timestamp());
    let package = entry
        .get("provider")
        .and_then(|provider| provider.get("npm"))
        .and_then(Value::as_str);
    Some(ModelInfo {
        id: id.to_owned(),
        name: entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| {
                *name != id && name.chars().count() <= MAX_NAME_CHARS && !name.chars().any(hidden)
            })
            .map(str::to_owned),
        context_window: entry
            .get("limit")
            .and_then(|limit| limit.get("context"))
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok())
            .filter(|tokens| {
                (crate::config::MIN_CONTEXT_WINDOW..=MAX_DESCRIBED_WINDOW).contains(tokens)
            }),
        prices,
        reasoning: entry.get("reasoning").and_then(Value::as_bool),
        created,
        api: Some(Api::of_package(package)),
    })
}

/// A character a terminal acts on or that changes how a name reads (zero-width marks, right-to-left overrides).
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// `url`'s answer as JSON, asked with the endpoint's key; the size bounded, the error shown without the key.
async fn get_json(
    client: &reqwest::Client,
    url: &str,
    endpoint: &ModelEndpoint,
    protocol: Protocol,
) -> Result<Value, String> {
    get_capped(client, url, endpoint, protocol, MAX_LIST_BYTES).await
}

/// [`get_json`] with its own bound on the answer's size.
async fn get_capped(
    client: &reqwest::Client,
    url: &str,
    endpoint: &ModelEndpoint,
    protocol: Protocol,
    limit: usize,
) -> Result<Value, String> {
    let mut request = client.get(url);
    if protocol == Protocol::AnthropicMessages {
        // Anthropic takes its key as `x-api-key`, with the API's version beside it.
        request = crate::anthropic::list_request(request, endpoint.api_key.as_deref());
    } else if let Some(key) = &endpoint.api_key {
        request = request.bearer_auth(key);
    }
    let mut response = request
        .send()
        .await
        .map_err(|err| format!("could not reach the provider: {}", err.without_url()))?;
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(format!("the list is larger than {limit} bytes"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| err.without_url().to_string())?
    {
        if body.len() + chunk.len() > limit {
            return Err(format!("the list is larger than {limit} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let text: String = String::from_utf8_lossy(&body).chars().take(300).collect();
        return Err(format!(
            "the provider answered {status}: {}",
            printable(&crate::model::redact(&text, endpoint.api_key.as_deref()))
        ));
    }
    serde_json::from_slice(&body).map_err(|err| format!("no model list: {err}"))
}

/// The kept list when it is fresh, else a new one from the provider — kept for next time; a provider that does not
/// answer leaves the old list in use.
///
/// # Errors
///
/// A message when there is no list at all: none kept and none fetched.
pub async fn current(
    home: &Path,
    id: &str,
    endpoint: &ModelEndpoint,
    protocol: Protocol,
) -> Result<ModelList, String> {
    let root = endpoint.base_url.trim_end_matches('/');
    let kept = cached(home, id, root);
    if let Some(list) = kept.as_ref().filter(|list| list.fresh(Utc::now())) {
        return Ok(list.clone());
    }
    match fetch(endpoint, protocol).await {
        Ok(list) => {
            // Not kept is not lost: the run has its list, the next one fetches again.
            let _ = keep(home, id, &list);
            Ok(list)
        }
        // A provider that does not answer leaves the kept list standing in for an hour, not asked on every run.
        Err(reason) => match kept {
            Some(mut list) => {
                list.failed_at = Some(Utc::now());
                let _ = keep(home, id, &list);
                Ok(list)
            }
            None => Err(reason),
        },
    }
}

/// Text a provider sent, as a terminal may show it: control characters (an escape sequence among them) become
/// spaces.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The models of an OpenAI-shaped list (`{"data": [{"id": …}, …]}`), with what OpenRouter, Groq and others add.
fn parse(list: &Value) -> Option<Vec<ModelInfo>> {
    let entries = list.get("data")?.as_array()?;
    let mut models: Vec<ModelInfo> = entries.iter().filter_map(model).collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Some(models)
}

fn model(entry: &Value) -> Option<ModelInfo> {
    let id = entry.get("id")?.as_str()?.trim();
    // An id a terminal would act on is no model's.
    if id.is_empty() || id.chars().any(char::is_control) {
        return None;
    }
    let context_window = ["context_length", "context_window", "max_context_length"]
        .iter()
        .find_map(|key| entry.get(*key).and_then(Value::as_u64))
        .and_then(|tokens| u32::try_from(tokens).ok())
        .filter(|tokens| *tokens > 0);
    let reasoning = entry
        .get("supported_parameters")
        .and_then(Value::as_array)
        .map(|names| names.iter().any(|name| name.as_str() == Some("reasoning")));
    Some(ModelInfo {
        id: id.to_owned(),
        name: entry
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|name| name != id),
        context_window,
        prices: entry.get("pricing").and_then(prices),
        reasoning,
        created: entry.get("created").and_then(Value::as_i64),
        api: None,
    })
}

/// OpenRouter's `pricing`: USD per token as decimal strings (`"0.0000001"`); a negative or missing input or output
/// price (a router whose price depends on the model it picks) is no price.
fn prices(pricing: &Value) -> Option<Prices> {
    let per_m = |key: &str| -> Option<f64> {
        let value = pricing.get(key)?;
        let per_token = match value {
            Value::String(text) => text.trim().parse::<f64>().ok()?,
            Value::Number(number) => number.as_f64()?,
            _ => return None,
        };
        let per_m = per_token * 1e6;
        (per_m.is_finite() && (0.0..=MAX_PRICE_PER_M).contains(&per_m)).then_some(per_m)
    };
    Some(Prices {
        input_per_m: per_m("prompt")?,
        cached_input_per_m: per_m("input_cache_read"),
        output_per_m: per_m("completion")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn models_dev_says_what_a_gateway_model_speaks_and_costs() {
        let entry = json!({"name": "GLM-5.3", "reasoning": true, "release_date": "2026-09-01",
            "limit": {"context": 1_000_000, "output": 131_072},
            "cost": {"input": 0.6, "output": 2.2, "cache_read": 0.11}});
        let glm = described_model("glm-5.3", &entry).unwrap();
        assert_eq!(
            glm.api,
            Some(Api::Chat),
            "no package: the provider's own, chat"
        );
        assert_eq!(glm.context_window, Some(1_000_000));
        assert_eq!(glm.reasoning, Some(true));
        let prices = glm.prices.unwrap();
        assert!((prices.input_per_m - 0.6).abs() < 1e-9);
        assert_eq!(prices.cached_input_per_m, Some(0.11));
        assert_eq!(glm.created, Some(1_788_220_800));
        let claude =
            json!({"provider": {"npm": "@ai-sdk/anthropic"}, "cost": {"input": -1, "output": 5}});
        let claude = described_model("claude-haiku-5-5", &claude).unwrap();
        assert_eq!(claude.api, Some(Api::Anthropic));
        assert_eq!(claude.prices, None, "a negative price is none");
        let spoofed = json!({"name": "GLM\u{202e}5", "limit": {"context": 99_999_999},
            "cost": {"input": 0, "output": 0}});
        let spoofed = described_model("glm-5.2", &spoofed).unwrap();
        assert_eq!(spoofed.name, None, "a right-to-left override is no name");
        assert_eq!(spoofed.context_window, None, "an absurd window is none");
        assert_eq!(
            spoofed.prices, None,
            "nothing both ways is no price for a model not called free"
        );
        let free = json!({"cost": {"input": 0, "output": 0}});
        assert!(described_model("glm-5.3-flash-free", &free).unwrap().free());
        let gpt = json!({"provider": {"npm": "@ai-sdk/openai"}});
        assert_eq!(
            described_model("gpt-6-luna", &gpt).unwrap().api,
            Some(Api::Responses)
        );
        assert!(!Api::Responses.spoken() && Api::Anthropic.spoken());
        assert_eq!(Api::guessed("muse-spark-1.3"), Api::Responses);
        assert_eq!(Api::guessed("kimi-k3"), Api::Chat);
        assert_eq!(Api::guessed("qwen3.7-max"), Api::Anthropic);
    }

    #[test]
    fn an_openrouter_list_gives_window_prices_and_reasoning() {
        let list = json!({"data": [
            {"id": "deepseek/deepseek-v4.1-flash", "name": "DeepSeek V4.1 Flash", "context_length": 163840,
             "pricing": {"prompt": "0.0000001", "completion": "0.0000004", "input_cache_read": "0.00000001"},
             "supported_parameters": ["tools", "reasoning"]},
            {"id": "openrouter/auto", "pricing": {"prompt": "-1", "completion": "-1"}},
            {"id": "qwen3:8b"},
            {"id": "  "},
            {"id": "qwen3:8b"},
            {"id": "evil\u{1b}[2J"},
            "not an object",
            {"id": "huge", "context_length": 99_999_999_999_u64,
             "pricing": {"prompt": "1e303", "completion": 0.000_001, "input_cache_read": "-1"}},
            {"id": "num", "pricing": {"prompt": 0.000_002, "completion": "NaN"}}
        ]});
        let models = parse(&list).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            [
                "deepseek/deepseek-v4.1-flash",
                "huge",
                "num",
                "openrouter/auto",
                "qwen3:8b"
            ],
            "an empty id, one with control characters and a non-object are dropped, a repeated one kept once"
        );
        let huge = models.iter().find(|m| m.id == "huge").unwrap();
        assert_eq!(huge.context_window, None, "beyond u32 is no window");
        assert_eq!(huge.prices, None, "an absurd input price is no price");
        let num = models.iter().find(|m| m.id == "num").unwrap();
        assert_eq!(num.prices, None, "NaN is no price");
        let flash = models
            .iter()
            .find(|m| m.id == "deepseek/deepseek-v4.1-flash")
            .unwrap();
        assert_eq!(flash.context_window, Some(163_840));
        assert_eq!(flash.reasoning, Some(true));
        let prices = flash.prices.unwrap();
        assert!((prices.input_per_m - 0.1).abs() < 1e-9);
        assert!((prices.output_per_m - 0.4).abs() < 1e-9);
        assert!((prices.cached_input_per_m.unwrap() - 0.01).abs() < 1e-9);
        let auto = models.iter().find(|m| m.id == "openrouter/auto").unwrap();
        assert_eq!(
            auto.prices, None,
            "a price that depends on the routed model is none"
        );
        let local = models.iter().find(|m| m.id == "qwen3:8b").unwrap();
        assert_eq!((local.context_window, local.reasoning), (None, None));
        assert!(parse(&json!({"models": []})).is_none());
        assert!(models.iter().any(|m| m.id == "openrouter/auto"));
        let free = ModelInfo {
            id: "x/y:free".into(),
            name: None,
            context_window: None,
            prices: None,
            reasoning: None,
            created: None,
            api: None,
        };
        assert!(free.free());
        assert!(!flash.free());
    }

    #[test]
    fn a_kept_list_is_its_roots_and_fresh_for_a_day() {
        let home = std::env::temp_dir().join(format!(
            "axh-models-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        let list = ModelList {
            format: FORMAT,
            fetched_at: Utc::now() - chrono::Duration::hours(2),
            failed_at: None,
            root: "https://openrouter.ai/api/v1".into(),
            models: vec![ModelInfo {
                id: "a/b".into(),
                name: Some("Model B".into()),
                context_window: None,
                prices: None,
                reasoning: None,
                created: Some(1_700_000_000),
                api: None,
            }],
            allowed: Some(vec!["a/b".into()]),
        };
        keep(&home, "openrouter", &list).unwrap();
        let back = cached(&home, "openrouter", "https://openrouter.ai/api/v1").unwrap();
        assert!(back.fresh(Utc::now()));
        let older = ModelList {
            format: 0,
            ..back.clone()
        };
        assert!(
            !older.fresh(Utc::now()),
            "a list of an older format is fetched again"
        );
        assert!(!back.fresh(Utc::now() + chrono::Duration::hours(23)));
        assert!(
            !back.fresh(Utc::now() - chrono::Duration::hours(3)),
            "a list from the future is not fresh"
        );
        let mut stale = back.clone();
        let later = Utc::now() + chrono::Duration::hours(23);
        stale.failed_at = Some(later);
        assert!(
            stale.fresh(later + chrono::Duration::minutes(30)),
            "a failed fetch lets the kept list stand in for an hour"
        );
        assert!(!stale.fresh(later + chrono::Duration::minutes(90)));
        std::fs::write(path(&home, "broken"), "{not json").unwrap();
        assert!(cached(&home, "broken", "https://openrouter.ai/api/v1").is_none());
        std::os::unix::fs::symlink(path(&home, "openrouter"), path(&home, "linked")).unwrap();
        assert!(
            cached(&home, "linked", "https://openrouter.ai/api/v1").is_none(),
            "a link is not followed"
        );
        assert!(cached(&home, "openrouter", "https://elsewhere.example/v1").is_none());
        assert_eq!(back.matching("model b").count(), 1);
        assert_eq!(back.matching("A/").count(), 1);
        assert_eq!(back.matching("b nope").count(), 0);
        assert!(
            back.allows("a/b") && !back.allows("c/d"),
            "only what the account may use"
        );
        assert_eq!(back.get("a/b").unwrap().created, Some(1_700_000_000));
    }
}
