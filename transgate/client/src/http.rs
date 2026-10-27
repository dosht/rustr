//! The one HTTP client every client call and GoTrue use.
//!
//! reqwest's `rustls` feature verifies certificates through
//! `rustls-platform-verifier`, which on Android needs JNI initialisation and
//! otherwise panics at the first TLS handshake. P8 D9 bundles the Mozilla roots
//! instead (`webpki-roots`) and hands reqwest a ready `rustls::ClientConfig`.
//! Every other target keeps reqwest's defaults (the OS trust store natively, the
//! browser's own stack on wasm32).

#[cfg(any(target_os = "android", test))]
fn bundled_roots_config() -> rustls::ClientConfig {
    use std::sync::Arc;

    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("the aws-lc-rs provider supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// A client for the API, GoTrue and asset fetches of this process.
#[cfg(target_os = "android")]
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .tls_backend_preconfigured(bundled_roots_config())
        .build()
        .expect("a client with bundled TLS roots builds")
}

/// A client for the API, GoTrue and asset fetches of this process.
#[cfg(not(target_os = "android"))]
pub fn http_client() -> reqwest::Client {
    reqwest::Client::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundle is the Mozilla root set, and reqwest accepts the config as a
    /// built rustls backend (any other type would silently become an unknown one).
    #[test]
    fn the_bundled_config_carries_the_mozilla_roots_and_builds_a_client() {
        let config = bundled_roots_config();
        assert!(!config.crypto_provider().cipher_suites.is_empty());
        assert!(webpki_roots::TLS_SERVER_ROOTS.len() > 100, "the root bundle is not empty");
        reqwest::Client::builder().tls_backend_preconfigured(config).build().expect("reqwest builds with the bundled config");
    }
}
