//! Product analytics as effects (P5 D12). Presentation decides which source
//! event fires and with which properties; the shell's telemetry adapter
//! forwards it to PostHog only after cookie consent and drops it otherwise.
//! Server transactional analytics is separate and unchanged.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Prop {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// One source analytics event: its exact source name and properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub name: Cow<'static, str>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<Cow<'static, str>, Prop>,
}

impl TelemetryEvent {
    pub const fn new(name: &'static str) -> TelemetryEvent {
        TelemetryEvent { name: Cow::Borrowed(name), properties: BTreeMap::new() }
    }

    pub fn text(mut self, key: &'static str, value: impl Into<String>) -> TelemetryEvent {
        self.properties.insert(Cow::Borrowed(key), Prop::Text(value.into()));
        self
    }

    pub fn number(mut self, key: &'static str, value: f64) -> TelemetryEvent {
        self.properties.insert(Cow::Borrowed(key), Prop::Number(value));
        self
    }

    pub fn flag(mut self, key: &'static str, value: bool) -> TelemetryEvent {
        self.properties.insert(Cow::Borrowed(key), Prop::Bool(value));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_serializes_its_source_name_and_typed_properties() {
        let e = TelemetryEvent::new("ai_chat_message_rated").text("rating", "up").number("index", 2.0).flag("mobile", true);
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"name": "ai_chat_message_rated", "properties": {"index": 2.0, "mobile": true, "rating": "up"}})
        );
        assert_eq!(serde_json::to_value(TelemetryEvent::new("loggedOut")).unwrap(), serde_json::json!({"name": "loggedOut"}));
    }
}
