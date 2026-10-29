#[path = "../src/asset_urls.rs"]
mod asset_urls;

use asset_urls::AssetUrls;

#[test]
fn every_document_asset_is_fetchable_from_a_nested_app_route() {
    let mut urls: AssetUrls = serde_json::from_value(serde_json::json!({
        "base": {
            "latin": "/_app/latin.ttf",
            "latinItalic": "/_app/italic.ttf",
            "arabic": "/_app/arabic.ttf",
            "symbols": "/_app/symbols.ttf",
            "arrows": "/_app/arrows.ttf",
            "invoiceLogo": "/_app/logo.jpg"
        },
        "fallbacks": {"cjk": "/_app/cjk.otf", "hebrew": "/_app/hebrew.ttf"}
    })).unwrap();
    urls.resolve("https://app.example.test/home/3");
    let client = reqwest::Client::new();
    for (name, actual) in [
        ("latin.ttf", &urls.base.latin), ("italic.ttf", &urls.base.latin_italic),
        ("arabic.ttf", &urls.base.arabic), ("symbols.ttf", &urls.base.symbols),
        ("arrows.ttf", &urls.base.arrows), ("logo.jpg", &urls.base.invoice_logo),
        ("cjk.otf", &urls.fallbacks["cjk"]), ("hebrew.ttf", &urls.fallbacks["hebrew"]),
    ] {
        let request = client.get(actual).build().unwrap_or_else(|error| panic!("{name} cannot be fetched: {error}"));
        assert_eq!(request.url().as_str(), format!("https://app.example.test/_app/{name}"));
    }
}
