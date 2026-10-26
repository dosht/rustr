use std::time::Duration;
use media_ports::{Headers, TransportError};
use reqwest::{Client, ClientBuilder, Response};
use serde::de::DeserializeOwned;
use subtle::ConstantTimeEq;
use url::Url;

#[derive(Clone, Debug)]
pub struct HttpConfig {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    /// Bound provider JSON/XML responses, not input audio duration or upload size.
    pub max_response_bytes: usize,
}
impl Default for HttpConfig {
    fn default() -> Self {
        Self { connect_timeout: Duration::from_secs(10), request_timeout: Duration::from_secs(60), max_response_bytes: 64 * 1024 * 1024 }
    }
}
pub(crate) fn validate(config: &HttpConfig) -> Result<(), TransportError> {
    if config.connect_timeout.is_zero() || config.request_timeout.is_zero() || config.max_response_bytes == 0 {
        return Err(TransportError::Configuration("nonzero HTTP limits required".into()));
    }
    Ok(())
}
pub(crate) fn builder(config: &HttpConfig) -> Result<ClientBuilder, TransportError> {
    validate(config)?;
    Ok(Client::builder().connect_timeout(config.connect_timeout).timeout(config.request_timeout)
        .read_timeout(config.request_timeout).redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never()).no_proxy().referer(false))
}
pub(crate) fn client(config: &HttpConfig) -> Result<Client, TransportError> {
    builder(config)?.build().map_err(|_| TransportError::Configuration("HTTP client".into()))
}
/// Only trusted configured origins use this exception for synthetic local HTTP fixtures.
/// Untrusted external audio URLs always use the stricter pinned-DNS fetcher.
pub(crate) fn configured_url(value: &str) -> Result<Url, TransportError> {
    let url = Url::parse(value).map_err(|_| TransportError::Configuration("endpoint URL".into()))?;
    let local = url.host_str().and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok()).is_some_and(|ip| ip.is_loopback());
    if url.host_str().is_none() || (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || url.query().is_some() {
        return Err(TransportError::Configuration("HTTPS endpoint without credentials/query required".into()));
    }
    Ok(url)
}
pub(crate) fn endpoint(base: &Url, segments: &[&str]) -> Result<Url, TransportError> {
    let mut url = base.clone();
    url.path_segments_mut().map_err(|_| TransportError::Configuration("endpoint path".into()))?
        .pop_if_empty().extend(segments.iter().copied());
    Ok(url)
}
pub(crate) fn unavailable(operation: &str, status: Option<u16>) -> TransportError {
    TransportError::Unavailable { status, operation: operation.into() }
}
pub(crate) async fn bytes(mut response: Response, limit: usize, operation: &str) -> Result<Vec<u8>, TransportError> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(TransportError::LimitExceeded(operation.into()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable(operation, None))? {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(TransportError::LimitExceeded(operation.into()));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
pub(crate) async fn json<T: DeserializeOwned>(response: Response, config: &HttpConfig, operation: &str) -> Result<T, TransportError> {
    let status = response.status();
    if !status.is_success() { return Err(unavailable(operation, Some(status.as_u16()))); }
    let body = bytes(response, config.max_response_bytes, operation).await?;
    serde_json::from_slice(&body).map_err(|_| TransportError::Malformed(operation.into()))
}
pub(crate) fn one_header<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    let mut matching = headers.iter().filter(|(key, _)| key.eq_ignore_ascii_case(name));
    let (_, value) = matching.next()?;
    if matching.next().is_some() { return None; }
    Some(value)
}
pub(crate) fn authentic(actual: Option<&str>, expected: &str) -> bool {
    !expected.is_empty() && actual.is_some_and(|value| bool::from(value.as_bytes().ct_eq(expected.as_bytes())))
}
