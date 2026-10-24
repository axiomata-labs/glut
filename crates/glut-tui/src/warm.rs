//! Keeping a resting session's prompt cache warm (`docs/plans/glut-tui.md`, T36e): while a session that has worked
//! rests, its last request goes out again about every four and a half minutes — answered as briefly as the provider
//! allows, read from its cache, which renews the entry — for at most half an hour. A new turn, a closed pane, another
//! session end it.

use std::sync::Arc;
use std::time::Duration;

use glut::{ChatModel, Message, Model, ToolSpec};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::session::Report;

/// How often a resting session's cache is read again: inside the five minutes a provider keeps it, with room for the
/// call itself.
pub const EVERY: Duration = Duration::from_secs(270);
/// How many times at most: about half an hour of rest, past which a cold start costs less than keeping on.
pub const TIMES: u32 = 6;
/// How long a cache lives after it was last read, the providers' common default.
pub const LIFE: Duration = Duration::from_secs(5 * 60);
/// How long Anthropic keeps it when asked for its hour.
pub const HOUR: Duration = Duration::from_secs(60 * 60);

/// How a session keeps its cache (`/cache`, kept in `prefs.json`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheMode {
    /// Read it again while resting — every provider (the default).
    #[default]
    Ping,
    /// Anthropic's one-hour life (each turn's cache write costs twice the input price); elsewhere as `Ping`.
    #[serde(rename = "1h")]
    Hour,
    /// Neither: a pause past the provider's lifetime starts cold.
    Off,
}

impl CacheMode {
    pub fn parse(word: &str) -> Option<CacheMode> {
        match word {
            "ping" => Some(CacheMode::Ping),
            "1h" | "hour" => Some(CacheMode::Hour),
            "off" => Some(CacheMode::Off),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CacheMode::Ping => "ping",
            CacheMode::Hour => "1h",
            CacheMode::Off => "off",
        }
    }
}

/// A keeper at work; dropping it ends it.
pub struct Keeper(tokio::task::JoinHandle<()>);

impl Drop for Keeper {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Keeps the cache of `messages` and `tools` on `model` warm while the session rests, each read's usage — or why it
/// stopped — reported as [`Report::Warmed`].
pub fn keep(
    model: Arc<Model>,
    messages: Vec<Message>,
    tools: Vec<ToolSpec>,
    host: Arc<dyn crate::host::Host>,
    reports: mpsc::UnboundedSender<Report>,
) -> Keeper {
    Keeper(tokio::spawn(async move {
        for _ in 0..TIMES {
            tokio::time::sleep(EVERY).await;
            // Each ping is a paid call: the host's budget has its word before every one.
            if let Err(reason) = host.may_spend() {
                let _ = reports.send(Report::Warmed(Err(reason)));
                return;
            }
            let warmed = model
                .warm(&messages, &tools)
                .await
                .map_err(|err| err.to_string());
            let failed = warmed.is_err();
            if reports.send(Report::Warmed(warmed)).is_err() || failed {
                return;
            }
        }
    }))
}

/// A choice in `prefs.json` this Glut does not know is no choice, rather than a prefs file whose other choices are
/// lost.
///
/// # Errors
///
/// Only what the reader itself fails at.
pub fn lenient<'de, D: serde::Deserializer<'de>>(reader: D) -> Result<Option<CacheMode>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(reader)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_read_as_typed_and_as_kept() {
        assert_eq!(CacheMode::parse("1h"), Some(CacheMode::Hour));
        assert_eq!(CacheMode::parse("ping"), Some(CacheMode::Ping));
        assert_eq!(CacheMode::parse("soon"), None);
        assert_eq!(serde_json::to_string(&CacheMode::Hour).unwrap(), "\"1h\"");
        assert_eq!(
            serde_json::from_str::<CacheMode>("\"off\"").unwrap(),
            CacheMode::Off
        );
    }

    #[test]
    fn half_an_hour_of_rest_at_most_inside_the_caches_life() {
        assert!(EVERY < LIFE);
        assert!(EVERY * TIMES <= Duration::from_secs(30 * 60));
    }
}
