//! The host's [`CallbackTransport`] (P7 D5, API-D06): core's request and outcome types over the
//! pinned-DNS poster of `media-adapters`. It decides nothing: classification, retry and the
//! schedule are core's.

use media_adapters::{CallbackConfig, CallbackPoster, PostOutcome};
use transgate_core::api::callbacks::{DELIVERY_ID_HEADER, EXCERPT_CHARS, SIGNATURE_HEADER, SendOutcome, TIMEOUT_SECONDS, USER_AGENT};
use transgate_core::api::ports::{CallbackRequest, CallbackTransport};
use transgate_core::jobs::ports::BoxFuture;

pub struct HostCallbackTransport {
    poster: CallbackPoster,
}

impl HostCallbackTransport {
    /// `allow_private` is the development and fixture switch; a deployment leaves it off.
    pub fn new(allow_private: bool) -> Self {
        Self::with_timeout(allow_private, std::time::Duration::from_secs(TIMEOUT_SECONDS))
    }

    pub fn with_timeout(allow_private: bool, timeout: std::time::Duration) -> Self {
        Self { poster: CallbackPoster::new(CallbackConfig { timeout, excerpt_chars: EXCERPT_CHARS, allow_private }) }
    }
}

impl CallbackTransport for HostCallbackTransport {
    fn post<'a>(&'a self, request: &'a CallbackRequest) -> BoxFuture<'a, SendOutcome> {
        Box::pin(async move {
            let delivery = request.delivery.0.to_string();
            let headers = [(DELIVERY_ID_HEADER, delivery.as_str()), (SIGNATURE_HEADER, request.signature.as_str()), ("user-agent", USER_AGENT)];
            match self.poster.post(&request.url, &headers, &request.body).await {
                PostOutcome::Status { code, excerpt } => SendOutcome::Status { code, excerpt },
                PostOutcome::Timeout => SendOutcome::Timeout,
                PostOutcome::Network(error) => SendOutcome::Network { error },
                PostOutcome::Refused(reason) => SendOutcome::Refused { reason },
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use transgate_core::api::callbacks::DeliveryId;
    use uuid::Uuid;

    /// One-shot loopback receiver that records headers and the body of what it was sent.
    async fn receiver(status: u16) -> (String, Arc<Mutex<Vec<(Vec<(String, String)>, Vec<u8>)>>>) {
        use axum::{Router, body::{Body, to_bytes}, extract::{Request, State}, response::Response};
        let seen: Seen = Arc::default();
        type Seen = Arc<Mutex<Vec<(Vec<(String, String)>, Vec<u8>)>>>;
        async fn handle(State((seen, status)): State<(Seen, u16)>, request: Request) -> Response {
            let (parts, body) = request.into_parts();
            let headers = parts.headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string())).collect();
            // Read the body first: a std mutex guard must not live across an await.
            let bytes = to_bytes(body, 1 << 20).await.unwrap().to_vec();
            seen.lock().unwrap().push((headers, bytes));
            Response::builder().status(status).body(Body::from("ok")).unwrap()
        }
        let app = Router::new().fallback(handle).with_state((seen.clone(), status));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    // The three D5 headers and the body bytes arrive exactly as core produced them.
    #[tokio::test]
    async fn the_d5_headers_and_the_signed_body_reach_the_receiver_unchanged() {
        let (url, seen) = receiver(202).await;
        let id = DeliveryId(Uuid::from_u128(7));
        let request = CallbackRequest { url, body: br#"{"job":{"id":1}}"#.to_vec(), delivery: id, signature: "t=5,v1=abc".into() };
        let outcome = HostCallbackTransport::new(true).post(&request).await;
        assert_eq!(outcome, SendOutcome::Status { code: 202, excerpt: "ok".into() });
        let (headers, body) = seen.lock().unwrap()[0].clone();
        let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        assert_eq!(header("transgate-delivery-id"), Some(id.0.to_string()));
        assert_eq!(header("transgate-signature"), Some("t=5,v1=abc".into()));
        assert_eq!(header("user-agent"), Some("Transgate-Callback/1.0".into()));
        assert_eq!(header("content-type"), Some("application/json".into()));
        assert_eq!(body, br#"{"job":{"id":1}}"#);
    }

    // Production default: loopback is refused and nothing is sent.
    #[tokio::test]
    async fn without_the_development_switch_a_loopback_receiver_is_refused_and_never_contacted() {
        let (url, seen) = receiver(200).await;
        let request = CallbackRequest { url, body: b"{}".to_vec(), delivery: DeliveryId(Uuid::from_u128(8)), signature: "t=1,v1=x".into() };
        assert!(matches!(HostCallbackTransport::new(false).post(&request).await, SendOutcome::Refused { .. }));
        assert!(seen.lock().unwrap().is_empty());
    }
}
