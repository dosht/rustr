//! Customer callback delivery wire (P7 D5, API-D06): one JSON POST to a customer-chosen URL.
//! The destination is resolved, every resolved address is validated as public, the validated
//! address is pinned for the connection, and the address actually connected to is checked
//! again after the answer. Redirects are never followed and nothing is retried here: the
//! caller (core's callback use case) owns the schedule and the final-versus-retry decision.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::Client;
use url::Url;

use crate::fetch::is_public_address;
use crate::http::{self, HttpConfig};

/// The most response body ever read for the delivery record excerpt, whatever the receiver sends.
const MAX_BODY_READ: usize = 8 * 1024;

#[derive(Clone, Debug)]
pub struct CallbackConfig {
    /// Total time for one POST, connection and answer included (API-016: 10 seconds).
    pub timeout: Duration,
    /// How many characters of the answer body are kept (API-017: 200).
    pub excerpt_chars: usize,
    /// Development and fixture switch: accept private and loopback destinations. A deployment
    /// leaves it off; only public addresses are ever connected to then.
    pub allow_private: bool,
}

impl Default for CallbackConfig {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(10), excerpt_chars: 200, allow_private: false }
    }
}

/// What one POST produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostOutcome {
    Status { code: u16, excerpt: String },
    Timeout,
    Network(String),
    /// The destination failed the send-time check. Nothing was sent.
    Refused(String),
}

pub struct CallbackPoster {
    config: CallbackConfig,
}

impl CallbackPoster {
    pub fn new(config: CallbackConfig) -> Self {
        Self { config }
    }

    /// Resolves and validates the destination: the host name (or literal) and the one address to pin.
    async fn destination(&self, url: &Url) -> Result<(String, SocketAddr), String> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err("only http and https destinations are allowed".into());
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err("credentials and fragments are not allowed in a callback URL".into());
        }
        let host = url.host_str().ok_or("the callback URL has no host")?.trim_matches(['[', ']']).to_owned();
        let port = url.port_or_known_default().ok_or("the callback URL has no port")?;
        let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            match tokio::time::timeout(self.config.timeout, tokio::net::lookup_host((host.as_str(), port))).await {
                Ok(Ok(found)) => found.collect(),
                // An unresolvable name is refused here, not "allowed after a DNS error" as the source did.
                Ok(Err(_)) | Err(_) => return Err("the callback host does not resolve".into()),
            }
        };
        if addresses.is_empty() {
            return Err("the callback host does not resolve".into());
        }
        if !self.config.allow_private && addresses.iter().any(|a| !is_public_address(a.ip())) {
            return Err("the callback destination is not a public address".into());
        }
        Ok((host, addresses[0]))
    }

    /// POSTs `body` with `headers`. A fresh pinned client per call: no pooled socket can carry a
    /// validation decision over to another destination.
    pub async fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> PostOutcome {
        let Ok(url) = Url::parse(url) else { return PostOutcome::Refused("the callback URL is not valid".into()) };
        let (host, address) = match self.destination(&url).await {
            Ok(found) => found,
            Err(reason) => return PostOutcome::Refused(reason),
        };
        let limits = HttpConfig { connect_timeout: self.config.timeout, request_timeout: self.config.timeout, max_response_bytes: MAX_BODY_READ };
        let client: Client = match http::builder(&limits).map(|b| b.resolve(&host, address).pool_max_idle_per_host(0).build()) {
            Ok(Ok(client)) => client,
            _ => return PostOutcome::Network("the HTTP client could not be built".into()),
        };
        let mut request = client.post(url).header("content-type", "application/json").body(body.to_vec());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = match request.send().await {
            Ok(response) => response,
            Err(e) if e.is_timeout() => return PostOutcome::Timeout,
            Err(e) => return PostOutcome::Network(e.without_url().to_string()),
        };
        // The address reached must be the one validated; anything else sent nothing useful and is refused.
        if response.remote_addr().is_none_or(|peer| peer.ip() != address.ip()) {
            return PostOutcome::Refused("the connected address is not the validated address".into());
        }
        let code = response.status().as_u16();
        let mut read: Vec<u8> = Vec::new();
        // Bounded read: a receiver cannot make a delivery buffer its whole reply.
        while read.len() < MAX_BODY_READ {
            match response.chunk().await {
                Ok(Some(chunk)) => read.extend_from_slice(&chunk[..chunk.len().min(MAX_BODY_READ - read.len())]),
                Ok(None) => break,
                // The status already arrived; a broken body only shortens the excerpt.
                Err(_) => break,
            }
        }
        let excerpt = String::from_utf8_lossy(&read).chars().take(self.config.excerpt_chars).collect();
        PostOutcome::Status { code, excerpt }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Reply, Server};

    fn poster(allow_private: bool) -> CallbackPoster {
        CallbackPoster::new(CallbackConfig { timeout: Duration::from_secs(2), excerpt_chars: 200, allow_private })
    }

    const HEADERS: [(&str, &str); 3] = [
        ("user-agent", "Transgate-Callback/1.0"),
        ("transgate-delivery-id", "d-1"),
        ("transgate-signature", "t=1,v1=ab"),
    ];

    #[tokio::test]
    async fn a_post_carries_the_body_unchanged_and_every_header_and_reports_the_status_and_excerpt() {
        let server = Server::start().await;
        server.reply(Reply::text(202, "accepted"));
        let body = br#"{"job":{"id":1}}"#;
        let outcome = poster(true).post(&format!("{}/hook?x=1", server.url), &HEADERS, body).await;
        assert_eq!(outcome, PostOutcome::Status { code: 202, excerpt: "accepted".into() });
        let seen = &server.requests()[0];
        assert_eq!((seen.method.as_str(), seen.path.as_str(), seen.query.as_str()), ("POST", "/hook", "x=1"));
        assert_eq!(seen.body, body);
        assert_eq!(seen.headers["content-type"], "application/json");
        assert_eq!(seen.headers["user-agent"], "Transgate-Callback/1.0");
        assert_eq!(seen.headers["transgate-delivery-id"], "d-1");
        assert_eq!(seen.headers["transgate-signature"], "t=1,v1=ab");
    }

    #[tokio::test]
    async fn statuses_are_reported_as_answered_not_interpreted() {
        for code in [200u16, 204, 301, 400, 408, 429, 500, 503] {
            let server = Server::start().await;
            server.reply(Reply::text(code, "x"));
            let outcome = poster(true).post(&format!("{}/", server.url), &HEADERS, b"{}").await;
            assert!(matches!(outcome, PostOutcome::Status { code: c, .. } if c == code), "{code}: {outcome:?}");
        }
    }

    // A customer cannot use a delivery to reach another host: the redirect target is never contacted.
    #[tokio::test]
    async fn a_redirect_is_reported_and_never_followed() {
        let (first, second) = (Server::start().await, Server::start().await);
        first.reply(Reply::text(307, "").header("location", &format!("{}/internal", second.url)));
        let outcome = poster(true).post(&format!("{}/", first.url), &HEADERS, b"{}").await;
        assert!(matches!(outcome, PostOutcome::Status { code: 307, .. }), "{outcome:?}");
        assert!(second.requests().is_empty(), "the redirect target received nothing");
        assert_eq!(first.requests().len(), 1);
    }

    #[tokio::test]
    async fn a_receiver_slower_than_the_bound_is_a_timeout_not_a_hang() {
        let server = Server::start().await;
        server.reply(Reply::text(200, "late").delayed(Duration::from_secs(5)));
        let slow = CallbackPoster::new(CallbackConfig { timeout: Duration::from_millis(300), ..CallbackConfig::default() });
        let started = std::time::Instant::now();
        let outcome = CallbackPoster { config: CallbackConfig { allow_private: true, ..slow.config.clone() } }
            .post(&format!("{}/", server.url), &HEADERS, b"{}")
            .await;
        assert_eq!(outcome, PostOutcome::Timeout);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_closed_port_is_a_network_failure() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let outcome = poster(true).post(&format!("http://{address}/"), &HEADERS, b"{}").await;
        assert!(matches!(outcome, PostOutcome::Network(_)), "{outcome:?}");
    }

    // API-D06: with the development switch off, nothing private is ever connected to, by literal or by name.
    #[tokio::test]
    async fn private_loopback_link_local_and_metadata_destinations_are_refused_before_anything_is_sent() {
        let server = Server::start().await;
        let local = server.url.clone();
        for target in [
            local.as_str(),
            "http://10.0.0.5/hook",
            "http://192.168.1.10:8080/hook",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]:9/hook",
            "http://[::ffff:127.0.0.1]:9/hook",
            "http://localhost:9/hook",
        ] {
            let outcome = poster(false).post(target, &HEADERS, b"{}").await;
            assert!(matches!(outcome, PostOutcome::Refused(_)), "{target}: {outcome:?}");
        }
        assert!(server.requests().is_empty(), "a refused destination is never contacted");
    }

    #[tokio::test]
    async fn only_http_and_https_without_credentials_are_destinations() {
        for target in ["ftp://example.test/x", "file:///etc/passwd", "http://user:pass@8.8.8.8/x", "http://8.8.8.8/x#frag", "not a url", "gopher://8.8.8.8/"] {
            let outcome = poster(true).post(target, &HEADERS, b"{}").await;
            assert!(matches!(outcome, PostOutcome::Refused(_)), "{target}: {outcome:?}");
        }
    }

    #[tokio::test]
    async fn an_unresolvable_name_is_refused_not_allowed_as_the_source_did() {
        let outcome = poster(false).post("http://no-such-host.invalid/hook", &HEADERS, b"{}").await;
        assert!(matches!(outcome, PostOutcome::Refused(_)), "{outcome:?}");
    }

    #[tokio::test]
    async fn a_huge_answer_is_cut_to_the_excerpt_without_buffering_it_all() {
        let server = Server::start().await;
        server.reply(Reply::text(500, &"é".repeat(2_000_000)));
        let outcome = poster(true).post(&format!("{}/", server.url), &HEADERS, b"{}").await;
        let PostOutcome::Status { code, excerpt } = outcome else { panic!("{outcome:?}") };
        assert_eq!(code, 500);
        assert_eq!(excerpt.chars().count(), 200, "characters, not bytes");
        assert!(excerpt.chars().all(|c| c == 'é'));
    }
}
