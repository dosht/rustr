use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use futures_util::StreamExt;
use media_ports::*;
use reqwest::{Method, Response};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use url::Url;
use crate::http::{self, HttpConfig};

#[derive(Clone, Debug)]
pub struct FetchConfig {
    pub http: HttpConfig,
    pub max_redirects: usize,
}
impl Default for FetchConfig {
    fn default() -> Self { Self { http:HttpConfig::default(), max_redirects:5 } }
}
/// No proxy, private-address exemption, development bypass or environment switch.
pub struct ControlledFetcher {
    config:FetchConfig,
    #[cfg(test)] fixture_address:Option<SocketAddr>,
}
fn public_v4(ip:Ipv4Addr) -> bool {
    let [a,b,c,_] = ip.octets();
    !(a == 0 || a == 10 || a == 127 || a >= 224 || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254) || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
        || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100))) || (a == 203 && b == 0 && c == 113))
}
fn public_v6(ip:Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() { return public_v4(mapped); }
    let s = ip.segments();
    // Global unicast only; exclude transition, benchmarking and documentation blocks.
    (s[0] & 0xe000) == 0x2000 && s[0] != 0x2002
        && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
        && !(s[0] == 0x3fff && (s[1] & 0xf000) == 0)
}
pub fn is_public_address(ip:IpAddr) -> bool {
    match ip { IpAddr::V4(ip) => public_v4(ip), IpAddr::V6(ip) => public_v6(ip) }
}
impl ControlledFetcher {
    pub fn new(config:FetchConfig) -> Result<Self, TransportError> {
        http::validate(&config.http)?;
        Ok(Self { config, #[cfg(test)] fixture_address:None })
    }
    async fn destination(&self, url:&Url) -> Result<(String, SocketAddr), TransportError> {
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some()
            || url.fragment().is_some() { return Err(TransportError::UnsafeDestination); }
        let host = url.host_str().ok_or(TransportError::UnsafeDestination)?.trim_matches(['[', ']']).to_owned();
        let port = url.port_or_known_default().ok_or(TransportError::UnsafeDestination)?;
        #[cfg(test)]
        if host == "fixture.invalid" && let Some(address) = self.fixture_address { return Ok((host,address)); }
        let addresses:Vec<_> = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![SocketAddr::new(ip,port)]
        } else {
            tokio::time::timeout(self.config.http.connect_timeout, tokio::net::lookup_host((host.as_str(),port)))
                .await.map_err(|_| TransportError::UnsafeDestination)?
                .map_err(|_| TransportError::UnsafeDestination)?.collect()
        };
        if addresses.is_empty() || addresses.iter().any(|address| !is_public_address(address.ip())) {
            return Err(TransportError::UnsafeDestination);
        }
        Ok((host,addresses[0]))
    }
    async fn request(&self, url:Url, method:Method, body:Option<&[u8]>) -> Result<Response, TransportError> {
        let (host,address) = self.destination(&url).await?;
        // Pin the validated address in the actual transport, not merely in a preceding HEAD.
        // A fresh client prevents an old pooled socket bypassing the validation decision.
        let client = http::builder(&self.config.http)?.resolve(&host,address).pool_max_idle_per_host(0)
            .build().map_err(|_| TransportError::Configuration("controlled HTTP client".into()))?;
        let mut request = client.request(method,url);
        if let Some(body) = body { request = request.header("content-type","application/json").body(body.to_owned()); }
        let response = request.send().await.map_err(|_| http::unavailable("controlled fetch",None))?;
        if response.remote_addr().is_none_or(|peer| peer.ip() != address.ip()) { return Err(TransportError::UnsafeDestination); }
        Ok(response)
    }
    async fn download(&self, source:&SensitiveUrl) -> Result<Response, TransportError> {
        let mut url = Url::parse(source.expose()).map_err(|_| TransportError::UnsafeDestination)?;
        for redirects in 0..=self.config.max_redirects {
            let response = self.request(url.clone(),Method::GET,None).await?;
            if matches!(response.status().as_u16(),301 | 302 | 303 | 307 | 308) {
                if redirects == self.config.max_redirects { return Err(TransportError::LimitExceeded("redirects".into())); }
                let location = response.headers().get("location").and_then(|h| h.to_str().ok()).ok_or(TransportError::UnsafeDestination)?;
                url = url.join(location).map_err(|_| TransportError::UnsafeDestination)?;
                continue;
            }
            if !response.status().is_success() { return Err(http::unavailable("external download",Some(response.status().as_u16()))); }
            return Ok(response);
        }
        Err(TransportError::LimitExceeded("redirects".into()))
    }
    /// Delivery workers use the same pinned-DNS policy. No redirect forwarding of customer payloads.
    pub async fn post_json(&self, target:&SensitiveUrl, body:&[u8]) -> Result<u16,TransportError> {
        let url = Url::parse(target.expose()).map_err(|_| TransportError::UnsafeDestination)?;
        let response = self.request(url,Method::POST,Some(body)).await?;
        Ok(response.status().as_u16())
    }
}
impl ExternalStager for ControlledFetcher {
    fn stage<'a>(&'a self, source:&'a SensitiveUrl, target:&'a ObjectRef, store:&'a dyn ObjectStore) -> BoxFuture<'a,Result<ObjectMetadata,TransportError>> {
        Box::pin(async move {
            if target.store != store.kind() || target.bucket != store.bucket() || target.version.is_some() {
                return Err(TransportError::Rejected { status:None,code:"staging_target".into() });
            }
            let mut response = self.download(source).await?;
            let content_type = response.headers().get("content-type").and_then(|h|h.to_str().ok()).unwrap_or("application/octet-stream").to_owned();
            // Anonymous spill file: bounded RAM and no persisted customer file name. Dropped on every error.
            // Spooling obtains a definite Content-Length, required by signed S3/R2 PUT.
            let file = tokio::task::spawn_blocking(tempfile::tempfile).await
                .map_err(|_| http::unavailable("staging file",None))?
                .map_err(|_| http::unavailable("staging file",None))?;
            let mut file = tokio::fs::File::from_std(file);
            let mut size = 0u64;
            while let Some(chunk) = response.chunk().await.map_err(|_| http::unavailable("external download body",None))? {
                size = size.checked_add(chunk.len() as u64).ok_or_else(|| TransportError::LimitExceeded("staging length".into()))?;
                file.write_all(&chunk).await.map_err(|_| http::unavailable("staging write",None))?;
            }
            file.flush().await.map_err(|_| http::unavailable("staging flush",None))?;
            file.rewind().await.map_err(|_| http::unavailable("staging rewind",None))?;
            let body = ReaderStream::new(file).map(|chunk|chunk.map_err(|_|http::unavailable("staging read",None)));
            store.put(PutObject { object:target.clone(),body:Box::pin(body),content_length:size,content_type }).await
        })
    }
}

#[cfg(test)]
mod tests;
