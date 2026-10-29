use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrls {
    pub latin: String,
    pub latin_italic: String,
    pub arabic: String,
    pub symbols: String,
    pub arrows: String,
    pub invoice_logo: String,
}

/// Keys are `FallbackFont` serde wire values (`hebrew`, …, `cjk`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUrls {
    pub base: BaseUrls,
    pub fallbacks: std::collections::BTreeMap<String, String>,
}

/// `/api` or `/auth/v1` resolve against the page; absolute values stay as given.
pub(crate) fn absolute(page: &str, value: &str) -> String {
    Url::parse(page).and_then(|base| base.join(value)).map(|u| u.to_string()).unwrap_or_else(|_| value.to_string()).trim_end_matches('/').to_string()
}

impl AssetUrls {
    pub fn resolve(&mut self, page: &str) {
        let BaseUrls { latin, latin_italic, arabic, symbols, arrows, invoice_logo } = &mut self.base;
        for url in [latin, latin_italic, arabic, symbols, arrows, invoice_logo]
            .into_iter()
            .chain(self.fallbacks.values_mut())
        {
            *url = absolute(page, url);
        }
    }
}
