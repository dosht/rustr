//! Same-origin licensed document assets for PDF export (D18). URLs come from the
//! web package's `?url` imports (p1V) through `TransgateConfig.documentAssets`.
//! Nothing is fetched at page load: the base set on the first PDF, an extra
//! script family only when a captured document needs it (CJK is 16 MB).
use serde::Deserialize;
use transgate_client::export::DocumentAssetSource;
use transgate_client::future::BoxFuture;
use transgate_export::documents::{DocumentAssets, FallbackFont, FontBytes};

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

pub struct FetchedAssets {
    http: reqwest::Client,
    urls: AssetUrls,
}

impl FetchedAssets {
    /// `urls` must already be absolute (resolved against the page by the presenter).
    pub fn new(urls: AssetUrls) -> Self {
        Self { http: reqwest::Client::new(), urls }
    }

    async fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let response = self.http.get(url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("{url}: HTTP {}", response.status()));
        }
        response.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
    }
}

impl DocumentAssetSource for FetchedAssets {
    fn base(&self) -> BoxFuture<'_, Result<DocumentAssets, String>> {
        Box::pin(async move {
            let b = &self.urls.base;
            let (latin, latin_italic, arabic, symbols, arrows, invoice_logo) =
                futures_util::try_join!(self.get(&b.latin), self.get(&b.latin_italic), self.get(&b.arabic), self.get(&b.symbols), self.get(&b.arrows), self.get(&b.invoice_logo))?;
            Ok(DocumentAssets { fonts: FontBytes { latin, latin_italic, arabic, symbols, arrows }, invoice_logo })
        })
    }

    fn fallback(&self, family: FallbackFont) -> BoxFuture<'_, Result<Vec<u8>, String>> {
        Box::pin(async move {
            let key = serde_json::to_value(family).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let url = self.urls.fallbacks.get(&key).ok_or_else(|| format!("no URL for {}", family.filename()))?;
            self.get(url).await
        })
    }
}
