//! Explicit real LLM composition for taskr; never provider fallback or a credential in a shell.

use std::sync::Arc;
use llm_core::Llm;
use llm_http::{DispatchCapacity, HttpLlm, ProviderConfig};

pub struct SummarySettings { pub provider: String, pub base_url: String, pub api_key: String, pub model: String }
impl std::fmt::Debug for SummarySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummarySettings").field("provider", &self.provider).field("model", &self.model).finish_non_exhaustive()
    }
}
impl Clone for SummarySettings {
    fn clone(&self) -> Self { Self { provider: self.provider.clone(), base_url: self.base_url.clone(), api_key: self.api_key.clone(), model: self.model.clone() } }
}

pub fn settings_from(get: &dyn Fn(&str) -> Option<String>) -> Result<Option<SummarySettings>, String> {
    if get("TASKR_SUMMARY_ENABLED").as_deref() != Some("1") { return Ok(None); }
    let required = |key: &str| get(key).filter(|v| !v.trim().is_empty()).ok_or_else(|| format!("{key} is required when TASKR_SUMMARY_ENABLED=1"));
    let provider = required("TASKR_SUMMARY_PROVIDER")?;
    let default_base = match provider.as_str() {
        "deepseek" => "https://api.deepseek.com",
        "openai" => "https://api.openai.com/v1",
        _ => return Err("TASKR_SUMMARY_PROVIDER must explicitly be deepseek or openai".into()),
    };
    Ok(Some(SummarySettings { provider, base_url: get("TASKR_SUMMARY_BASE_URL").unwrap_or_else(|| default_base.into()),
        api_key: required("TASKR_SUMMARY_API_KEY")?, model: required("TASKR_SUMMARY_MODEL")? }))
}

pub fn provider(settings: &SummarySettings, capacity: DispatchCapacity) -> Result<Arc<dyn Llm>, String> {
    let config = ProviderConfig::new(&settings.base_url, &settings.api_key);
    let llm = match settings.provider.as_str() {
        "deepseek" => HttpLlm::deepseek(config, capacity),
        "openai" => HttpLlm::openai(config, capacity),
        _ => return Err("TASKR_SUMMARY_PROVIDER is not configured".into()),
    }.map_err(|e| e.to_string())?;
    Ok(Arc::new(llm))
}
