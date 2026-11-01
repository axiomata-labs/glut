//! Which hosts serve a model on OpenRouter (T90): OpenRouter serves one model from many hosts at different
//! quantizations, prices and speeds, and takes a host order with every request. Glut reads a model's hosts
//! (`/api/v1/models/{author}/{slug}/endpoints`, kept a day) and ranks them by the owner's rule — full precision before
//! `fp8` unless it costs too much more, `fp4` last, a slow or unreliable host to the end — and a model's own pins
//! come before the rule. The order goes with fallbacks allowed, so a request never fails for the rule.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// How long a model's list of hosts is kept before it is read again.
const KEPT_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// How long a failed read is remembered: offline, or for a model OpenRouter lists no hosts of, every start would
/// otherwise wait for it again.
const MISSED_FOR: Duration = Duration::from_secs(10 * 60);
/// How long reading a model's hosts may take before the request goes without an order.
const READ_WAIT: Duration = Duration::from_secs(5);

/// The quantizations OpenRouter takes in `provider.quantizations`: any other word fails every request.
pub const QUANTIZATIONS: &[&str] = &[
    "int4", "int8", "fp4", "fp6", "fp8", "fp16", "bf16", "fp32", "unknown",
];

/// The owner's rule (`/route rule …`), kept with the provider in `providers.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    /// Whether Glut sends an order at all; off, OpenRouter alone decides.
    pub on: bool,
    /// How much more a more precise tier may cost than the one below it, as a fraction (0.4: 40 %).
    pub margin: f64,
    /// The uptime over the last day a host needs, in percent, not to go to the end.
    pub uptime: f64,
    /// The tokens a second a host needs (the median of the last 30 minutes).
    pub tokens_per_second: f64,
    /// The most seconds to the first token (the median of the last 30 minutes).
    pub first_token_seconds: f64,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            on: true,
            margin: 0.4,
            uptime: 95.0,
            tokens_per_second: 30.0,
            first_token_seconds: 10.0,
        }
    }
}

/// A model's own pins (`/route prefer|avoid|quant|strict`): they come before the rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pins {
    /// Hosts first, in this order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefer: Vec<String>,
    /// Hosts never.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub avoid: Vec<String>,
    /// The quantizations allowed; empty: any.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub quant: Vec<String>,
    /// The preferred hosts alone, no other as a fallback.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strict: bool,
}

impl Routing {
    pub fn is_default(&self) -> bool {
        *self == Routing::default()
    }

    /// Whether every value can go into a request — `providers.json` may have been written by hand.
    ///
    /// # Errors
    ///
    /// What is wrong, for the owner.
    pub fn check(&self) -> Result<(), String> {
        if let Some(rule) = &self.rule {
            let fine = |value: f64| value.is_finite() && value >= 0.0;
            if !(fine(rule.margin)
                && fine(rule.tokens_per_second)
                && fine(rule.first_token_seconds)
                && fine(rule.uptime)
                && rule.uptime <= 100.0)
            {
                return Err("routing.rule: numbers at or above 0, an uptime of at most 100".into());
            }
        }
        for (model, pins) in &self.models {
            if let Some(word) = pins
                .quant
                .iter()
                .find(|word| !QUANTIZATIONS.contains(&word.as_str()))
            {
                return Err(format!(
                    "routing.models.{model}.quant: {word:?} is none of {}",
                    QUANTIZATIONS.join(", ")
                ));
            }
            if let Some(name) = pins
                .prefer
                .iter()
                .chain(&pins.avoid)
                .find(|name| !is_host_name(name))
            {
                return Err(format!(
                    "routing.models.{model}: {name:?} is no host's name"
                ));
            }
        }
        Ok(())
    }
}

/// Whether `name` can name a host (`deepinfra`, `xiaomi/fp8`).
pub fn is_host_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 60
        && name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '/' | '.')
        })
}

impl Pins {
    pub fn is_empty(&self) -> bool {
        *self == Pins::default()
    }
}

/// A provider's routing as `providers.json` keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Routing {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<Rule>,
    /// Pins by model id.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, Pins>,
}

/// One host serving a model, as OpenRouter lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    /// The slug a request names it by (`xiaomi/fp8`, `deepinfra`).
    pub tag: String,
    /// Its name as OpenRouter shows it ("Xiaomi").
    pub name: String,
    pub quantization: Option<String>,
    /// USD per million prompt and completion tokens together; `None` where OpenRouter names no price above 0.
    pub price: Option<f64>,
    /// Percent over the last day, where measured.
    pub uptime: Option<f64>,
    /// Whether it serves now (OpenRouter's status 0).
    pub up: bool,
    /// The median tokens a second and seconds to the first token over the last 30 minutes, where measured.
    pub tokens_per_second: Option<f64>,
    pub first_token_seconds: Option<f64>,
}

/// How precise a host's quantization is, the higher the better; `None`: it names none.
fn tier(quantization: Option<&str>) -> Option<u8> {
    match quantization? {
        "bf16" | "fp16" | "fp32" => Some(3),
        "fp8" | "mxfp8" | "int8" | "fp6" => Some(2),
        "fp4" | "mxfp4" | "nvfp4" | "int4" => Some(0),
        _ => None,
    }
}

/// A host's tier for the rule: one that names none sits between `fp8` and `fp4`, the model's maker's own host counts
/// as full precision.
fn ranked_tier(host: &Host, maker: &str) -> u8 {
    let own = host.tag.split('/').next() == Some(maker);
    match tier(host.quantization.as_deref()) {
        Some(tier) => tier,
        None if own => 3,
        None => 1,
    }
}

/// A host's price where it has one above 0: a missing price must not make it the cheapest.
fn priced(host: &Host) -> Option<f64> {
    host.price.filter(|price| *price > 0.0)
}

/// Whether a host is fast and reliable enough by `rule`; an unmeasured value counts as enough.
fn fit(host: &Host, rule: &Rule) -> bool {
    host.up
        && host.uptime.is_none_or(|uptime| uptime >= rule.uptime)
        && host
            .tokens_per_second
            .is_none_or(|speed| speed >= rule.tokens_per_second)
        && host
            .first_token_seconds
            .is_none_or(|wait| wait <= rule.first_token_seconds)
}

/// Why a host stands where the rule put it; each side words it itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Standing {
    /// In the tier the rule chose.
    Chosen,
    /// Another tier, above or below.
    Other,
    /// Too slow, down or unreliable now: at the end.
    Slow,
}

/// The hosts of `model` (`author/slug`) in the order the rule gives, each with why it stands there.
pub fn rank(hosts: &[Host], model: &str, rule: &Rule) -> Vec<(String, Standing)> {
    let maker = model.split('/').next().unwrap_or_default();
    let (fit_hosts, rest): (Vec<&Host>, Vec<&Host>) =
        hosts.iter().partition(|host| fit(host, rule));
    let pool = if fit_hosts.is_empty() {
        &rest
    } else {
        &fit_hosts
    };
    // The cheapest price of each tier among the hosts the rule weighs.
    let mut cheapest: BTreeMap<u8, f64> = BTreeMap::new();
    for host in pool {
        let tier = ranked_tier(host, maker);
        let price = cheapest.entry(tier).or_insert(f64::INFINITY);
        *price = price.min(priced(host).unwrap_or(f64::INFINITY));
    }
    // The most precise tier whose cheapest host costs at most `margin` more than the cheapest of the tier below it;
    // fp4 only where nothing else serves. The margin weighs named tiers alone: a host that names none may be fp4, and
    // being cheap must not put it before fp8.
    let tiers: Vec<u8> = cheapest.keys().rev().copied().collect();
    let mut chosen = tiers.iter().copied().find(|tier| *tier > 0);
    for pair in tiers.windows(2) {
        let (upper, lower) = (pair[0], pair[1]);
        if lower <= 1 {
            break;
        }
        if Some(upper) != chosen {
            continue;
        }
        if cheapest[&upper] > cheapest[&lower] * (1.0 + rule.margin) {
            chosen = Some(lower);
        }
    }
    let chosen = chosen.or(tiers.last().copied()).unwrap_or(0);
    let mut ordered: Vec<(&Host, u8, bool)> = hosts
        .iter()
        .map(|host| {
            (
                host,
                ranked_tier(host, maker),
                fit(host, rule) || fit_hosts.is_empty(),
            )
        })
        .collect();
    // Fit first; then the chosen tier, then the more precise ones above it, then down the tiers; cheaper first.
    let place = |tier: u8| -> u8 {
        if tier == chosen {
            0
        } else if tier > chosen {
            1
        } else {
            2 + (chosen - tier)
        }
    };
    ordered.sort_by(|a, b| {
        (!a.2, place(a.1)).cmp(&(!b.2, place(b.1))).then_with(|| {
            let (a, b) = (priced(a.0), priced(b.0));
            (a.is_none(), a.unwrap_or_default())
                .partial_cmp(&(b.is_none(), b.unwrap_or_default()))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    ordered
        .into_iter()
        .map(|(host, tier, fit)| {
            let standing = if !fit {
                Standing::Slow
            } else if tier == chosen {
                Standing::Chosen
            } else {
                Standing::Other
            };
            (host.tag.clone(), standing)
        })
        .collect()
}

/// The `provider` object of a request for `model`: the pins first, then the rule's order; `None` where neither says
/// anything.
pub fn provider_object(hosts: &[Host], model: &str, rule: &Rule, pins: &Pins) -> Option<Value> {
    if pins.is_empty() && !rule.on {
        return None;
    }
    let mut order: Vec<String> = pins.prefer.clone();
    // Strict means the preferred hosts alone: with no fallback allowed, every host in the order may serve.
    let alone = pins.strict && !pins.prefer.is_empty();
    if rule.on && !alone {
        for (tag, _) in rank(hosts, model, rule) {
            let named =
                |pinned: &String| tag == *pinned || tag.split('/').next() == Some(pinned.as_str());
            if !order.iter().any(named) && !pins.avoid.iter().any(named) {
                order.push(tag);
            }
        }
    }
    let mut provider = json!({"allow_fallbacks": !pins.strict});
    if !order.is_empty() {
        provider["order"] = json!(order);
    }
    if !pins.avoid.is_empty() {
        provider["ignore"] = json!(pins.avoid);
    }
    if !pins.quant.is_empty() {
        provider["quantizations"] = json!(pins.quant);
    }
    Some(provider)
}

/// The hosts OpenRouter lists for `model`, as its endpoints answer reads.
pub fn hosts_of(answer: &Value) -> Vec<Host> {
    let price = |pricing: &Value, key: &str| {
        pricing
            .get(key)
            .and_then(Value::as_str)
            .and_then(|price| price.parse::<f64>().ok())
            .filter(|price| price.is_finite())
    };
    answer
        .pointer("/data/endpoints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|endpoint| {
            let pricing = endpoint.get("pricing").cloned().unwrap_or(Value::Null);
            Some(Host {
                tag: endpoint.get("tag")?.as_str()?.to_owned(),
                name: endpoint
                    .get("provider_name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                quantization: endpoint
                    .get("quantization")
                    .and_then(Value::as_str)
                    .filter(|level| *level != "unknown")
                    .map(str::to_owned),
                // Per token in the list; per million here. Routers list -1, a missing price is no price.
                price: match (price(&pricing, "prompt"), price(&pricing, "completion")) {
                    (Some(prompt), Some(completion)) if prompt >= 0.0 && completion >= 0.0 => {
                        Some((prompt + completion) * 1_000_000.0).filter(|price| *price > 0.0)
                    }
                    _ => None,
                },
                uptime: endpoint.get("uptime_last_1d").and_then(Value::as_f64),
                up: endpoint.get("status").and_then(Value::as_i64).unwrap_or(0) == 0,
                tokens_per_second: endpoint
                    .pointer("/throughput_last_30m/p50")
                    .and_then(Value::as_f64),
                first_token_seconds: endpoint
                    .pointer("/latency_last_30m/p50")
                    .and_then(Value::as_f64)
                    .map(|ms| ms / 1000.0),
            })
        })
        .collect()
}

/// Whether `root` is OpenRouter's: only it serves a model from hosts a request may choose.
pub fn is_openrouter(root: &str) -> bool {
    crate::model::is_openrouter(root)
}

/// The rule and the pins of `model` as `home`'s `providers.json` keeps them for provider `id`.
fn rule_and_pins(home: &Path, id: &str, model: &str) -> (Rule, Pins) {
    let routing = super::Providers::load(home)
        .ok()
        .and_then(|list| list.providers.get(id).map(|entry| entry.routing.clone()))
        .unwrap_or_default();
    let pins = routing.models.get(model).cloned().unwrap_or_default();
    (routing.rule.unwrap_or_default(), pins)
}

/// The `provider` object for `endpoint` of provider `id` as `home`'s `providers.json` keeps its routing: the rule's
/// order and the model's pins — `None` for a provider that is not OpenRouter, or with nothing to say.
pub async fn for_endpoint(
    home: &Path,
    id: &str,
    endpoint: &crate::config::ModelEndpoint,
) -> Option<Value> {
    if !is_openrouter(&endpoint.base_url) {
        return None;
    }
    let (rule, pins) = rule_and_pins(home, id, &endpoint.model);
    let hosts = if rule.on {
        hosts(
            home,
            &endpoint.base_url,
            endpoint.api_key.as_deref(),
            &endpoint.model,
        )
        .await
    } else {
        Vec::new()
    };
    provider_object(&hosts, &endpoint.model, &rule, &pins)
}

/// [`for_endpoint`] from the hosts already kept, reading nothing: a changed pin takes effect before the next message.
pub fn kept_for_endpoint(
    home: &Path,
    id: &str,
    endpoint: &crate::config::ModelEndpoint,
) -> Option<Value> {
    if !is_openrouter(&endpoint.base_url) {
        return None;
    }
    let (rule, pins) = rule_and_pins(home, id, &endpoint.model);
    let hosts = kept_hosts(home, &endpoint.model).unwrap_or_default();
    provider_object(&hosts, &endpoint.model, &rule, &pins)
}

/// The hosts of `model` as last read, however old; `None` where none were read yet.
pub fn kept_hosts(home: &Path, model: &str) -> Option<Vec<Host>> {
    let text = std::fs::read_to_string(kept_at(home, model)).ok()?;
    serde_json::from_str::<Vec<Host>>(&text)
        .ok()
        .filter(|hosts| !hosts.is_empty())
}

/// Where a model's hosts are kept.
fn kept_at(home: &Path, model: &str) -> PathBuf {
    let name: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    home.join(".glut")
        .join("models")
        .join("hosts")
        .join(format!("{name}.json"))
}

/// The hosts of `model` on OpenRouter at `root` (its API root, `…/api/v1`): kept a day, else read with `key` —
/// without one the list has no speeds; an empty list where they cannot be read, remembered a while.
pub async fn hosts(home: &Path, root: &str, key: Option<&str>, model: &str) -> Vec<Host> {
    let path = kept_at(home, model);
    let age = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok());
    if let Some(age) = age
        && let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(hosts) = serde_json::from_str::<Vec<Host>>(&text)
        && age
            < if hosts.is_empty() {
                MISSED_FOR
            } else {
                KEPT_FOR
            }
    {
        return hosts;
    }
    let hosts = read_hosts(root, key, model).await;
    if let Some(dir) = path.parent()
        && super::make_private_dir(dir).is_ok()
        && let Ok(text) = serde_json::to_string(&hosts)
    {
        let _ = super::write_replacing(dir, &path, text.as_bytes(), 0o600);
    }
    hosts
}

/// Reads the hosts of `model` from OpenRouter's endpoints list; an empty list where it cannot.
async fn read_hosts(root: &str, key: Option<&str>, model: &str) -> Vec<Host> {
    let Some((author, slug)) = model.split_once('/') else {
        return Vec::new();
    };
    // A model's own variant (`:free`) is listed under its base.
    let slug = slug.split(':').next().unwrap_or(slug);
    // Each part one path segment: a `..` or a `/` would ask another of OpenRouter's pages.
    let segment = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    // The key goes to OpenRouter alone, over https, and follows no redirect elsewhere.
    if !root.starts_with("https://") || !is_openrouter(root) || !segment(author) || !segment(slug) {
        return Vec::new();
    }
    let url = format!(
        "{}/models/{author}/{slug}/endpoints",
        root.trim_end_matches('/')
    );
    let Ok(client) = reqwest::Client::builder()
        .timeout(READ_WAIT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return Vec::new();
    };
    let mut request = client.get(&url);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let Ok(response) = request.send().await else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(text) = response.text().await else {
        return Vec::new();
    };
    serde_json::from_str::<Value>(&text)
        .map(|answer| hosts_of(&answer))
        .unwrap_or_default()
}

/// What `/route` and `glut providers route` were asked to change (T91: one reading for both).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// The model's pins away.
    Off,
    Prefer(Vec<String>),
    Avoid(Vec<String>),
    Quant(Vec<String>),
    Strict(bool),
    RuleOn(bool),
    /// In percent, as the owner says it.
    Margin(f64),
    Uptime(f64),
    Speed(f64),
    Wait(f64),
}

/// Why a route could not be read or kept; each side words it itself.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteError {
    /// The words are no change `/route` knows.
    Usage,
    /// A quantization OpenRouter does not take.
    Quant,
    /// The provider is not OpenRouter: it has no hosts to choose.
    NotHere,
    /// No such provider in `providers.json`.
    NoProvider,
    /// `providers.json` could not be read or written.
    File(String),
}

/// Reads the words after `route`: `Ok(None)` is no change, only the showing.
///
/// # Errors
///
/// [`RouteError::Usage`] for words it does not know, [`RouteError::Quant`] for a level OpenRouter does not take.
pub fn parse(words: &[&str]) -> Result<Option<Change>, RouteError> {
    let names = |words: &[&str]| -> Result<Vec<String>, RouteError> {
        words
            .iter()
            .map(|word| word.to_lowercase())
            .map(|word| is_host_name(&word).then_some(word).ok_or(RouteError::Usage))
            .collect()
    };
    let number = |word: &str| -> Result<f64, RouteError> {
        word.trim_end_matches('%')
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite() && *number >= 0.0)
            .ok_or(RouteError::Usage)
    };
    Ok(Some(match words {
        [] => return Ok(None),
        ["off"] => Change::Off,
        ["prefer", hosts @ ..] if !hosts.is_empty() => Change::Prefer(names(hosts)?),
        ["avoid", hosts @ ..] if !hosts.is_empty() => Change::Avoid(names(hosts)?),
        ["quant", levels @ ..] if !levels.is_empty() => {
            let levels = names(levels)?;
            if levels
                .iter()
                .any(|level| !QUANTIZATIONS.contains(&level.as_str()))
            {
                return Err(RouteError::Quant);
            }
            Change::Quant(levels)
        }
        ["strict", switch @ ("on" | "off")] => Change::Strict(*switch == "on"),
        ["rule", switch @ ("on" | "off")] => Change::RuleOn(*switch == "on"),
        ["rule", "margin", n] => Change::Margin(number(n)?),
        ["rule", "uptime", n] => Change::Uptime(number(n)?.min(100.0)),
        ["rule", "speed", n] => Change::Speed(number(n)?),
        ["rule", "wait", n] => Change::Wait(number(n)?),
        _ => return Err(RouteError::Usage),
    }))
}

/// One host as `/route` shows it: in the rule's order, with why it stands there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ranked {
    pub tag: String,
    /// The quantization it names, if any.
    pub quantization: Option<String>,
    pub standing: Standing,
    /// USD per million tokens, where OpenRouter names a price.
    pub price: Option<f64>,
    pub tokens_per_second: Option<f64>,
}

/// What `/route` shows of a model: the rule, its pins and, where the hosts were read and the rule is on, their order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shown {
    pub model: String,
    pub rule: Rule,
    pub pins: Pins,
    /// `None`: no hosts read yet; empty: the rule is off.
    pub hosts: Option<Vec<Ranked>>,
}

/// The provider `id`'s entry for `root`, if it is OpenRouter's.
fn openrouter_entry<'a>(
    list: &'a mut super::Providers,
    id: &str,
    root: &str,
) -> Result<&'a mut super::Entry, RouteError> {
    if !is_openrouter(root) {
        return Err(RouteError::NotHere);
    }
    list.providers.get_mut(id).ok_or(RouteError::NoProvider)
}

/// What `/route` shows for `model` of provider `id` at `root`, from the hosts already kept.
///
/// # Errors
///
/// [`RouteError::NotHere`] off OpenRouter, [`RouteError::NoProvider`], [`RouteError::File`].
pub fn show(home: &Path, id: &str, root: &str, model: &str) -> Result<Shown, RouteError> {
    let mut list = super::Providers::load(home).map_err(|err| RouteError::File(err.to_string()))?;
    let entry = openrouter_entry(&mut list, id, root)?;
    let rule = entry.routing.rule.clone().unwrap_or_default();
    let pins = entry.routing.models.get(model).cloned().unwrap_or_default();
    let hosts = kept_hosts(home, model).map(|hosts| {
        if !rule.on {
            return Vec::new();
        }
        rank(&hosts, model, &rule)
            .into_iter()
            .map(|(tag, standing)| {
                let host = hosts.iter().find(|host| host.tag == tag);
                Ranked {
                    quantization: host.and_then(|host| host.quantization.clone()),
                    price: host.and_then(|host| host.price),
                    tokens_per_second: host.and_then(|host| host.tokens_per_second),
                    tag,
                    standing,
                }
            })
            .collect()
    });
    Ok(Shown {
        model: model.to_owned(),
        rule,
        pins,
        hosts,
    })
}

/// Keeps `change` for `model` of provider `id` at `root` in `providers.json`; the caller sets the model's routing
/// anew after it ([`kept_for_endpoint`]).
///
/// # Errors
///
/// [`RouteError::NotHere`] off OpenRouter, [`RouteError::NoProvider`], [`RouteError::File`].
pub fn apply(
    home: &Path,
    id: &str,
    root: &str,
    model: &str,
    change: Change,
) -> Result<(), RouteError> {
    let mut list = super::Providers::load(home).map_err(|err| RouteError::File(err.to_string()))?;
    let entry = openrouter_entry(&mut list, id, root)?;
    let mut rule = entry.routing.rule.clone().unwrap_or_default();
    let pins = entry.routing.models.entry(model.to_owned()).or_default();
    match change {
        Change::Off => *pins = Pins::default(),
        Change::Prefer(hosts) => pins.prefer = hosts,
        Change::Avoid(hosts) => pins.avoid = hosts,
        Change::Quant(levels) => pins.quant = levels,
        Change::Strict(on) => pins.strict = on,
        Change::RuleOn(on) => rule.on = on,
        Change::Margin(percent) => rule.margin = percent / 100.0,
        Change::Uptime(percent) => rule.uptime = percent,
        Change::Speed(speed) => rule.tokens_per_second = speed,
        Change::Wait(seconds) => rule.first_token_seconds = seconds,
    }
    if pins.is_empty() {
        entry.routing.models.remove(model);
    }
    entry.routing.rule = (rule != Rule::default()).then_some(rule);
    list.save(home)
        .map_err(|err| RouteError::File(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(tag: &str, quantization: Option<&str>, price: f64) -> Host {
        Host {
            tag: tag.into(),
            name: tag.into(),
            quantization: quantization.map(str::to_owned),
            price: Some(price),
            uptime: Some(99.0),
            up: true,
            tokens_per_second: Some(60.0),
            first_token_seconds: Some(1.0),
        }
    }

    fn tags(ranked: &[(String, Standing)]) -> Vec<&str> {
        ranked.iter().map(|(tag, _)| tag.as_str()).collect()
    }

    #[test]
    fn full_precision_wins_unless_it_costs_too_much_more_and_fp4_comes_last() {
        let rule = Rule::default();
        let hosts = [
            host("cheap4/fp4", Some("fp4"), 0.5),
            host("eight/fp8", Some("fp8"), 1.0),
            host("full/bf16", Some("bf16"), 1.3),
            host("silent", None, 0.9),
        ];
        assert_eq!(
            tags(&rank(&hosts, "maker/m", &rule)),
            ["full/bf16", "eight/fp8", "silent", "cheap4/fp4"]
        );
        // 60 % dearer than fp8: fp8 first, full precision after it, fp4 still last.
        let dear = [
            hosts[0].clone(),
            hosts[1].clone(),
            host("full/bf16", Some("bf16"), 1.6),
        ];
        assert_eq!(
            tags(&rank(&dear, "maker/m", &rule)),
            ["eight/fp8", "full/bf16", "cheap4/fp4"]
        );
    }

    #[test]
    fn a_slow_or_down_host_goes_to_the_end_and_the_makers_own_counts_as_full() {
        let rule = Rule::default();
        let mut slow = host("full/bf16", Some("bf16"), 1.0);
        slow.tokens_per_second = Some(10.0);
        let mut down = host("down/fp8", Some("fp8"), 0.8);
        down.up = false;
        let hosts = [
            slow,
            down,
            host("eight/fp8", Some("fp8"), 1.0),
            host("maker", None, 1.2),
        ];
        assert_eq!(
            tags(&rank(&hosts, "maker/m", &rule)),
            ["maker", "eight/fp8", "full/bf16", "down/fp8"]
        );
    }

    #[test]
    fn pins_come_first_and_the_object_never_forbids_a_fallback_unless_strict() {
        let rule = Rule::default();
        let hosts = [
            host("a/fp8", Some("fp8"), 1.0),
            host("b/bf16", Some("bf16"), 1.0),
            host("c/fp4", Some("fp4"), 0.1),
        ];
        let pins = Pins {
            prefer: vec!["a".into()],
            avoid: vec!["c".into()],
            ..Pins::default()
        };
        let object = provider_object(&hosts, "x/m", &rule, &pins).unwrap();
        assert_eq!(object["order"], json!(["a", "b/bf16"]));
        assert_eq!(object["ignore"], json!(["c"]));
        assert_eq!(object["allow_fallbacks"], json!(true));
        let off = Rule { on: false, ..rule };
        assert!(provider_object(&hosts, "x/m", &off, &Pins::default()).is_none());
        let strict = Pins {
            strict: true,
            ..pins
        };
        assert_eq!(
            provider_object(&hosts, "x/m", &off, &strict).unwrap()["allow_fallbacks"],
            json!(false)
        );
    }

    #[test]
    fn openrouters_list_is_read_in_its_own_units() {
        let answer = json!({"data": {"endpoints": [{
            "tag": "xiaomi/fp8", "provider_name": "Xiaomi", "quantization": "fp8", "status": 0,
            "pricing": {"prompt": "0.0000001", "completion": "0.0000004"}, "uptime_last_1d": 99.5,
            "throughput_last_30m": {"p50": 80.0}, "latency_last_30m": {"p50": 900.0},
        }, {"tag": "cohere", "provider_name": "Cohere", "quantization": "unknown", "status": -1, "pricing": {}}]}});
        let hosts = hosts_of(&answer);
        assert_eq!(hosts.len(), 2);
        assert!((hosts[0].price.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(hosts[1].price, None);
        assert_eq!(hosts[0].first_token_seconds, Some(0.9));
        assert_eq!(hosts[1].quantization, None);
        assert!(!hosts[1].up);
    }

    #[test]
    fn strict_sends_the_preferred_hosts_alone() {
        let hosts = [
            host("a/fp8", Some("fp8"), 1.0),
            host("b/fp4", Some("fp4"), 0.1),
        ];
        let pins = Pins {
            prefer: vec!["a".into()],
            strict: true,
            ..Pins::default()
        };
        let object = provider_object(&hosts, "x/m", &Rule::default(), &pins).unwrap();
        assert_eq!(object["order"], json!(["a"]));
        assert_eq!(object["allow_fallbacks"], json!(false));
    }

    #[test]
    fn a_host_without_a_price_is_never_the_cheapest_and_an_unnamed_one_never_beats_fp8() {
        let rule = Rule::default();
        let mut free = host("free/bf16", Some("bf16"), 0.0);
        free.price = None;
        let hosts = [
            host("full/bf16", Some("bf16"), 1.2),
            free,
            host("eight/fp8", Some("fp8"), 1.0),
            host("silent", None, 0.5),
        ];
        assert_eq!(
            tags(&rank(&hosts, "maker/m", &rule)),
            ["full/bf16", "free/bf16", "eight/fp8", "silent"]
        );
    }

    #[test]
    fn a_hand_written_routing_is_checked() {
        let mut routing = Routing::default();
        assert!(routing.check().is_ok());
        routing.models.insert(
            "x/m".into(),
            Pins {
                quant: vec!["full".into()],
                ..Pins::default()
            },
        );
        assert!(routing.check().is_err());
        routing.models.clear();
        routing.rule = Some(Rule {
            uptime: 120.0,
            ..Rule::default()
        });
        assert!(routing.check().is_err());
    }

    #[test]
    fn the_route_words_are_read_once_for_every_side() {
        assert_eq!(parse(&[]), Ok(None));
        assert_eq!(
            parse(&["prefer", "Xiaomi", "deepinfra/fp8"]),
            Ok(Some(Change::Prefer(vec![
                "xiaomi".into(),
                "deepinfra/fp8".into()
            ])))
        );
        assert_eq!(
            parse(&["quant", "fp8", "bf16"]),
            Ok(Some(Change::Quant(vec!["fp8".into(), "bf16".into()])))
        );
        assert_eq!(parse(&["quant", "full"]), Err(RouteError::Quant));
        assert_eq!(
            parse(&["rule", "uptime", "120"]),
            Ok(Some(Change::Uptime(100.0)))
        );
        assert_eq!(parse(&["rule", "margin", "-5"]), Err(RouteError::Usage));
        assert_eq!(parse(&["prefer"]), Err(RouteError::Usage));
        assert_eq!(parse(&["avoid", "a b;c"]), Err(RouteError::Usage));
    }

    #[test]
    fn a_change_is_kept_for_its_model_and_off_openrouter_there_is_none() {
        let home = std::env::temp_dir().join(format!(
            "glut-route-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        let mut list = super::super::Providers::default();
        list.providers.insert(
            "openrouter".into(),
            serde_json::from_value(json!({"kind": "openrouter", "model": "x/m"})).unwrap(),
        );
        list.save(&home).unwrap();
        let root = "https://openrouter.ai/api/v1";
        apply(
            &home,
            "openrouter",
            root,
            "x/m",
            Change::Avoid(vec!["c".into()]),
        )
        .unwrap();
        apply(&home, "openrouter", root, "x/m", Change::Margin(25.0)).unwrap();
        let shown = show(&home, "openrouter", root, "x/m").unwrap();
        assert_eq!(shown.pins.avoid, ["c"]);
        assert!((shown.rule.margin - 0.25).abs() < 1e-9);
        assert_eq!(shown.hosts, None, "nothing read yet");
        apply(&home, "openrouter", root, "x/m", Change::Off).unwrap();
        let kept = super::super::Providers::load(&home).unwrap();
        assert!(kept.providers["openrouter"].routing.models.is_empty());
        assert_eq!(
            apply(
                &home,
                "openrouter",
                "https://api.anthropic.com",
                "x/m",
                Change::Off
            ),
            Err(RouteError::NotHere)
        );
        assert_eq!(
            apply(&home, "nobody", root, "x/m", Change::Off),
            Err(RouteError::NoProvider)
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
