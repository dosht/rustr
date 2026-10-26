//! Payment attribution as the source collected it: first-touch UTM parameters
//! and referrer kept in storage (`utils/analytics/utmCapture.ts`), plus device
//! type and browser from the user agent (`deviceDetection.ts`). Pure functions;
//! the shell supplies the URL search, referrer, user agent and clock.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The source's storage key, kept so attribution captured before cutover on the same origin survives.
pub const UTM_STORAGE_KEY: &str = "transgate_utm_data";

const CAMPAIGN_KEYS: [&str; 5] = ["utm_source", "utm_medium", "utm_campaign", "utm_content", "utm_term"];

/// The stored first-touch record, in the source's camelCase JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UtmData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_medium: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_campaign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utm_term: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

/// `captureUTM`: the JSON to store, or `None` to leave storage alone. First touch
/// wins (an existing record is never replaced); nothing is stored without a UTM
/// parameter or a referrer. Empty values count as absent, as `|| undefined` did.
pub fn capture(existing: Option<&str>, search: &str, referrer: &str, now_iso: &str) -> Option<String> {
    if existing.is_some_and(|e| !e.is_empty()) {
        return None;
    }
    let params: Vec<(String, String)> = url::form_urlencoded::parse(search.trim_start_matches('?').as_bytes()).into_owned().collect();
    let has_utm = CAMPAIGN_KEYS.iter().any(|key| params.iter().any(|(k, _)| k == key));
    if !has_utm && referrer.is_empty() {
        return None;
    }
    let get = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    let data = UtmData {
        utm_source: get("utm_source"),
        utm_medium: get("utm_medium"),
        utm_campaign: get("utm_campaign"),
        utm_content: get("utm_content"),
        utm_term: get("utm_term"),
        referrer: (!referrer.is_empty()).then(|| referrer.to_string()),
        captured_at: Some(now_iso.to_string()),
    };
    Some(serde_json::to_string(&data).expect("plain struct"))
}

/// `getDeviceType`: tablet first (tablets often say "mobile" too), then mobile.
pub fn device_type(user_agent: &str) -> &'static str {
    let ua = user_agent.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| ua.contains(w));
    if any(&["ipad", "tablet", "playbook", "silk"]) || (ua.contains("android") && !ua.contains("mobile")) {
        return "tablet";
    }
    if any(&["mobile", "iphone", "ipod", "android", "blackberry", "opera mini", "iemobile", "wpdesktop"]) {
        return "mobile";
    }
    "desktop"
}

/// `getBrowser`, in the source's order: Firefox, Edge, Chrome, Safari, Opera, IE.
pub fn browser(user_agent: &str) -> &'static str {
    let has = |word: &str| user_agent.contains(word);
    if has("Firefox") {
        "Firefox"
    } else if has("Edg") {
        "Edge"
    } else if has("Chrome") {
        "Chrome"
    } else if has("Safari") {
        "Safari"
    } else if has("Opera") || has("OPR") {
        "Opera"
    } else if has("MSIE") || has("Trident") {
        "IE"
    } else {
        "unknown"
    }
}

/// The snake_case attribution the source sent with checkout and `checkout_started`:
/// stored first touch plus device type and browser. Absent values are omitted.
pub fn checkout_attribution(stored: Option<&str>, user_agent: &str) -> Map<String, Value> {
    let data: UtmData = stored.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    let mut out = Map::new();
    let mut put = |key: &str, value: Option<String>| {
        if let Some(value) = value {
            out.insert(key.into(), Value::String(value));
        }
    };
    put("utm_source", data.utm_source);
    put("utm_medium", data.utm_medium);
    put("utm_campaign", data.utm_campaign);
    put("utm_content", data.utm_content);
    put("utm_term", data.utm_term);
    put("referrer", data.referrer);
    put("device_type", Some(device_type(user_agent).into()));
    put("browser", Some(browser(user_agent).into()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
    const ANDROID_TABLET: &str = "Mozilla/5.0 (Linux; Android 14; SM-X710) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";
    const EDGE: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36 Edg/126.0";

    #[test]
    fn user_agent_heuristics_follow_the_source_order() {
        assert_eq!((device_type(IPHONE), browser(IPHONE)), ("mobile", "Safari"));
        assert_eq!((device_type(ANDROID_TABLET), browser(ANDROID_TABLET)), ("tablet", "Chrome"));
        assert_eq!((device_type(EDGE), browser(EDGE)), ("desktop", "Edge"), "Edge says Chrome too; Edg is checked first");
    }

    #[test]
    fn first_touch_is_never_replaced_and_nothing_is_stored_without_a_signal() {
        let first = capture(None, "?utm_source=google&utm_medium=cpc&utm_term=", "", "2026-10-25T10:00:00.000Z").unwrap();
        let stored: UtmData = serde_json::from_str(&first).unwrap();
        assert_eq!(stored.utm_source.as_deref(), Some("google"));
        assert_eq!(stored.utm_term, None, "an empty value is absent");
        assert_eq!(capture(Some(&first), "?utm_source=bing", "", "later"), None);
        assert_eq!(capture(None, "?view=new", "", "now"), None);
        assert!(capture(None, "", "https://news.example/", "now").is_some(), "a referrer alone is a first touch");
    }

    #[test]
    fn checkout_attribution_sends_snake_case_and_omits_absent_values() {
        let stored = capture(None, "?utm_source=newsletter", "https://ref.example/", "now");
        let attribution = checkout_attribution(stored.as_deref(), IPHONE);
        assert_eq!(
            Value::Object(attribution),
            serde_json::json!({"utm_source": "newsletter", "referrer": "https://ref.example/", "device_type": "mobile", "browser": "Safari"})
        );
    }
}
