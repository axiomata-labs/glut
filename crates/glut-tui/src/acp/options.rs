//! The session's settings as the editor's pickers (T64b): its mode, its model and its thinking level as
//! `configOptions` — the whole list every time, so the levels follow the model. v2 keeps them as they are.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::Ordering;

use glut::Effort;
use glut::permissions::Mode;
use glut::providers::{Providers, models};
use serde_json::{Value, json};

use super::{Live, MODES};
use crate::session::{self, Engine};
use crate::store::Session;

/// The ids of the three pickers, as the editor hands them back.
pub const MODE: &str = "mode";
pub const MODEL: &str = "model";
pub const EFFORT: &str = "effort";
/// The thinking level's value that leaves it to the owner's settings and the provider.
const DEFAULT_EFFORT: &str = "default";
/// How many models used lately join each provider's own in the list.
const MOST_RECENT: usize = 8;

/// Every picker with its value now; the files the models come from are read off the runtime's workers.
pub async fn list(live: &Live) -> Value {
    let mode = live
        .parts
        .tools
        .gate()
        .map_or(Mode::Default, |gate| gate.mode());
    let engine = live
        .engine
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    let home = live.parts.home.clone();
    let (model, effort) = tokio::task::spawn_blocking(move || {
        (model_option(&home, &engine), effort_option(&home, &engine))
    })
    .await
    .unwrap_or((Value::Null, None));
    let mut options = vec![mode_option(mode)];
    if !model.is_null() {
        options.push(model);
    }
    options.extend(effort);
    Value::Array(options)
}

fn mode_option(mode: Mode) -> Value {
    let modes: Vec<Value> = MODES
        .iter()
        .map(
            |(mode, name, about)| json!({"value": mode.name(), "name": name, "description": about}),
        )
        .collect();
    json!({
        "id": MODE, "name": tr!(mode_title), "category": "mode", "type": "select",
        "currentValue": mode.name(), "options": modes,
    })
}

/// A model's value: its provider and its id, as `/model` takes them.
pub fn model_value(provider: &str, model: &str) -> String {
    format!("{provider} {model}")
}

/// The models by provider: each provider's own, then the ones the owner's sessions used lately, the one in use
/// always among them — any other by `/model <provider> <model>`.
fn model_option(home: &Path, engine: &Engine) -> Value {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Whether the model joined the list, not there already.
    let add = |groups: &mut BTreeMap<String, Vec<String>>, provider: &str, model: &str| {
        let models = groups.entry(provider.to_owned()).or_default();
        let new = !models.iter().any(|known| known == model);
        if new {
            models.push(model.to_owned());
        }
        new
    };
    add(&mut groups, &engine.provider, engine.model_name());
    let providers = Providers::load(home).ok();
    if let Some(list) = &providers {
        for (id, entry) in &list.providers {
            if let Some(model) = &entry.model {
                add(&mut groups, id, model);
            }
        }
    }
    let mut heads = Session::heads(home);
    heads.sort_by_key(|head| std::cmp::Reverse(head.updated));
    let mut recent = 0;
    for head in heads {
        if recent == MOST_RECENT {
            break;
        }
        let (Some(provider), Some(model)) = (head.provider, head.model) else {
            continue;
        };
        // Only a provider still set up: a model of one removed would fail to start.
        let known = providers
            .as_ref()
            .is_some_and(|list| list.providers.contains_key(&provider));
        if known && add(&mut groups, &provider, &model) {
            recent += 1;
        }
    }
    let shown = |text: &str| crate::transcript::clean(text).to_string();
    let grouped: Vec<Value> = groups
        .iter()
        .map(|(provider, models)| {
            let options: Vec<Value> = models
                .iter()
                .map(|model| json!({"value": model_value(provider, model), "name": shown(model)}))
                .collect();
            json!({"group": provider, "name": shown(provider), "options": options})
        })
        .collect();
    json!({
        "id": MODEL, "name": tr!(model_title), "category": "model", "type": "select",
        "currentValue": model_value(&engine.provider, engine.model_name()), "options": grouped,
    })
}

/// The thinking levels, unless the provider's list says the model does not reason.
fn effort_option(home: &Path, engine: &Engine) -> Option<Value> {
    let reasons = Providers::load(home)
        .ok()
        .and_then(|list| {
            let entry = list.providers.get(&engine.provider)?.clone();
            models::cached(home, &engine.provider, entry.root().trim_end_matches('/'))
        })
        .and_then(|list| {
            list.models
                .iter()
                .find(|model| model.id == engine.model_name())
                .and_then(|model| model.reasoning)
        });
    if reasons == Some(false) {
        return None;
    }
    let mut levels = vec![json!({
        "value": DEFAULT_EFFORT, "name": tr!(effort_default), "description": tr!(effort_default_detail),
    })];
    levels.extend(
        Effort::ALL
            .iter()
            .map(|effort| json!({"value": effort.as_str(), "name": effort.as_str()})),
    );
    let current = engine
        .endpoint
        .effort
        .map_or(DEFAULT_EFFORT, Effort::as_str);
    Some(json!({
        "id": EFFORT, "name": tr!(effort_title), "category": "thought_level", "type": "select",
        "currentValue": current, "options": levels,
    }))
}

/// Whether `value` is one of the picker's own: an editor may only pick what it was offered.
fn offered(options: &Value, id: &str, value: &str) -> bool {
    let Some(option) = options
        .as_array()
        .and_then(|options| options.iter().find(|option| option["id"] == id))
    else {
        return false;
    };
    let values = option["options"].as_array().into_iter().flatten();
    values
        .flat_map(
            |entry| match entry.get("options").and_then(Value::as_array) {
                Some(grouped) => grouped.iter().collect::<Vec<_>>(),
                None => vec![entry],
            },
        )
        .any(|entry| entry["value"] == value)
}

/// What the editor picked, made so; the notes to say beside it. A turn holds the engine: the caller refuses a model
/// or a level while one runs.
pub async fn set(live: &Live, id: &str, value: &str) -> Result<Vec<String>, String> {
    if !offered(&list(live).await, id, value) {
        return Err(format!("{value} is not one of {id}'s values"));
    }
    match id {
        MODE => {
            let mode = Mode::parse(value).ok_or_else(|| format!("no mode {value}"))?;
            super::set_mode(live, mode).map(|warning| warning.into_iter().collect())
        }
        MODEL => {
            let (provider, model) = value
                .split_once(' ')
                .ok_or_else(|| format!("no model {value}"))?;
            let effort = live
                .engine
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .endpoint
                .effort;
            switch(live, provider, model, Some(effort)).await
        }
        EFFORT => {
            let effort = match value {
                DEFAULT_EFFORT => None,
                level => Some(Effort::parse_or_explain(level)?),
            };
            let (provider, model) = {
                let engine = live
                    .engine
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                (engine.provider.clone(), engine.model_name().to_owned())
            };
            switch(live, &provider, &model, Some(effort)).await
        }
        other => Err(format!("Glut has no setting {other}")),
    }
}

/// Builds the engine of `provider`'s `model` at `effort` and goes on with it, kept as the next start's choice as
/// the TUI keeps it; what the build had to say comes back.
pub async fn switch(
    live: &Live,
    provider: &str,
    model: &str,
    effort: Option<Option<Effort>>,
) -> Result<Vec<String>, String> {
    if live.parts.fixed_engine {
        return Err(tr!(model_locked).to_string());
    }
    // Built in a task of its own, as the TUI builds it: a panic in it is a failed switch, not a lost answer.
    let (parts, provider, model) = (
        std::sync::Arc::clone(&live.parts),
        provider.to_owned(),
        model.to_owned(),
    );
    let (engine, mut notes) = tokio::spawn(async move {
        session::engine(&parts, Some(&provider), Some(&model), effort).await
    })
    .await
    .unwrap_or_else(|err| Err(tr!(switch_broke_off, reason = err)))?;
    if let Err(reason) = session::keep_choice(&live.parts.home, &engine) {
        notes.push(tr!(choice_unkept, reason = reason).to_string());
    }
    {
        let mut kept = live
            .kept
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        kept.provider = engine.provider.clone();
        kept.model = engine.model_name().to_owned();
    }
    notes.push(
        tr!(
            model_now,
            provider = engine.provider,
            model = engine.model_name()
        )
        .to_string(),
    );
    *live
        .engine
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = engine;
    live.context_used.store(0, Ordering::SeqCst);
    // Kept at once: an editor closed right after the switch opens the session on the new model.
    live.save().await;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_offered_value_is_taken_grouped_or_flat() {
        let options = json!([
            {"id": "mode", "options": [{"value": "default"}, {"value": "plan"}]},
            {"id": "model", "options": [
                {"group": "or", "options": [{"value": "or a/b"}]},
                {"group": "ollama", "options": [{"value": "ollama qwen"}]},
            ]},
        ]);
        assert!(offered(&options, "mode", "plan"));
        assert!(!offered(&options, "mode", "dont_ask"));
        assert!(offered(&options, "model", "ollama qwen"));
        assert!(!offered(&options, "model", "or c/d"));
        assert!(!offered(&options, "effort", "high"));
    }
}
