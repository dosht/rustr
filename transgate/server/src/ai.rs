//! Transgate's live AI composition. The source analysis/chat/summary call sites
//! select DeepSeek explicitly; OpenAI remains an explicit shared/taskr adapter,
//! never a fallback or an implicit model substitution here.
use std::sync::Arc;

use billing_core::types::Clock;
use llm_core::Llm;
use llm_http::{DispatchCapacity, HttpLlm, ProviderConfig};
use media_ports::SecretString;
use transgate_core::ai::{Ai, AiConfig, AiServices, Bounds, ChatLimits, ModelSettings};
use transgate_core::ai::live::LiveHub;
use transgate_core::jobs::ports::{JobStore, Sleeper};

#[derive(Clone, Debug)]
pub struct Settings {
    pub policy: AiConfig,
    pub base_url: String,
    pub api_key: Option<SecretString>,
}

impl Settings {
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        let api_key = match get("DEEPSEEK_API_KEY") {
            Some(key) if key.trim().is_empty() => return Err("DEEPSEEK_API_KEY must not be blank".into()),
            Some(key) => Some(SecretString::new(key)),
            None => None,
        };
        if get("DEEPSEEK_BASE_URL").is_some() && api_key.is_none() {
            return Err("DEEPSEEK_API_KEY is required when DEEPSEEK_BASE_URL is configured".into());
        }
        Ok(Self {
            policy: AiConfig {
                features_enabled: get("ENABLE_AI_FEATURES").as_deref() == Some("true"),
                chat_enabled: get("ENABLE_AI_CHAT").as_deref() == Some("true"),
                chat_validation: get("AI_CHAT_VALIDATION_ENABLED").as_deref() != Some("false"),
                limits: ChatLimits::from_env(get),
                model: ModelSettings::source(),
                bounds: Bounds::default(),
            },
            base_url: get("DEEPSEEK_BASE_URL").unwrap_or_else(|| "https://api.deepseek.com".into()),
            api_key,
        })
    }
}

pub fn compose(settings: Settings, store: Arc<dyn JobStore>, clock: Arc<dyn Clock>, sleeper: Arc<dyn Sleeper>) -> Result<Arc<Ai>, String> {
    let llm: Option<Arc<dyn Llm>> = settings.api_key.as_ref().map(|key| {
        HttpLlm::deepseek(ProviderConfig::new(&settings.base_url, key.expose()), DispatchCapacity::default())
            .map(|llm| Arc::new(llm) as Arc<dyn Llm>).map_err(|e| e.to_string())
    }).transpose()?;
    Ok(Arc::new(Ai::new(AiServices {
        store,
        clock,
        llm,
        sleeper,
        live: Arc::new(LiveHub::new(64 * 1024, 1024 * 1024)),
    }, settings.policy)))
}
