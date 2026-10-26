//! Transgate's billing identity: a configurable JWT verifier (a shared HS256
//! secret and/or a JWKS endpoint) and an explicit, opt-in development token.
//!
//! There is no anonymous fallback and no "the bearer string is the user". A
//! missing, malformed, unsigned, expired or wrongly signed credential is
//! `Unauthenticated`. The development token is honored only when the process
//! was started with it deliberately enabled, and it never bypasses the
//! verification of real tokens.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, header};
use billing_core::types::{AccountId, BoxFuture};
use billing_http::{AuthError, BillingIdentity, Caller};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::RwLock;
use uuid::Uuid;

const AUDIENCE: &str = "authenticated";
/// A fetched key set is trusted for at most this long; after it the set is
/// fetched again, and a key the provider no longer lists stops verifying.
const JWKS_MAX_AGE: Duration = Duration::from_secs(600);
/// An unknown key id triggers a refresh at most this often.
const JWKS_REFRESH_AT_MOST_EVERY: Duration = Duration::from_secs(60);
pub const DEV_PREFIX: &str = "dev:";

/// Namespace for subjects that are not UUIDs (development tokens): a retry
/// reaches the same account. Real UUID subjects are used as they are.
const SUBJECTS: Uuid = Uuid::from_u128(0x7a3c_1d52_9b0e_4f6a_8c21_5e94_0b77_d3a1);

#[derive(Deserialize)]
struct Claims {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    user_metadata: Option<Metadata>,
}

#[derive(Deserialize)]
struct Metadata {
    full_name: Option<String>,
}

pub struct JwtIdentity {
    hs256: Option<DecodingKey>,
    jwks_url: Option<String>,
    /// The last fetched key set and when it was fetched.
    jwks: RwLock<Option<(JwkSet, Instant)>>,
    http: reqwest::Client,
    dev_tokens: bool,
    admin_emails: BTreeSet<String>,
    /// `Instant::now`, replaced by tests to move time deterministically.
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl JwtIdentity {
    /// `hs256_secret` and/or `jwks_url` configure real verification. `dev_tokens`
    /// must be an explicit decision of whoever starts the process.
    pub fn new(
        hs256_secret: Option<&str>,
        jwks_url: Option<String>,
        dev_tokens: bool,
        admin_emails: BTreeSet<String>,
    ) -> Result<JwtIdentity, String> {
        if hs256_secret.is_none() && jwks_url.is_none() && !dev_tokens {
            return Err("configure SUPABASE_JWT_SECRET or SUPABASE_JWKS_URL (or TRANSGATE_DEV_AUTH=1 locally)".into());
        }
        Ok(JwtIdentity {
            hs256: hs256_secret.map(|s| DecodingKey::from_secret(s.as_bytes())),
            jwks_url,
            jwks: RwLock::new(None),
            http: reqwest::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|e| e.to_string())?,
            dev_tokens,
            admin_emails: admin_emails.into_iter().map(|e| e.to_lowercase()).collect(),
            now: Box::new(Instant::now),
        })
    }

    fn caller(&self, subject: &str, email: &str, full_name: Option<String>) -> Caller {
        Caller {
            account: AccountId::from_subject(subject, &SUBJECTS),
            email: email.to_string(),
            full_name,
            is_admin: self.admin_emails.contains(&email.to_lowercase()),
        }
    }

    fn age(&self, fetched_at: Instant) -> Duration {
        (self.now)().saturating_duration_since(fetched_at)
    }

    async fn key_for(&self, kid: &str) -> Result<DecodingKey, AuthError> {
        let invalid = |e: String| AuthError::Unauthenticated(e);
        let url = self
            .jwks_url
            .as_deref()
            .ok_or_else(|| invalid("asymmetric token but no JWKS configured".into()))?;
        let decoding = |jwk: &Jwk| DecodingKey::from_jwk(jwk).map_err(|e| invalid(e.to_string()));
        let unknown = || invalid(format!("unknown signing key {kid}"));
        // A cached key is trusted only while the set it came from is within its lifetime.
        {
            let cache = self.jwks.read().await;
            if let Some((set, fetched_at)) = cache.as_ref() && self.age(*fetched_at) < JWKS_MAX_AGE && let Some(jwk) = set.find(kid) {
                return decoding(jwk);
            }
        }
        // One refresh at a time; requests that waited here use its result.
        let mut cache = self.jwks.write().await;
        if let Some((set, fetched_at)) = cache.as_ref() {
            let age = self.age(*fetched_at);
            if age < JWKS_MAX_AGE {
                if let Some(jwk) = set.find(kid) {
                    return decoding(jwk);
                }
                if age < JWKS_REFRESH_AT_MOST_EVERY {
                    return Err(unknown());
                }
            }
        }
        // A failed refresh is reported as such; a key past its lifetime is never used instead.
        let set: JwkSet = self
            .http
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AuthError::Unavailable(format!("JWKS: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::Unavailable(format!("JWKS: {e}")))?;
        let found = set.find(kid).map(decoding);
        *cache = Some((set, (self.now)()));
        found.unwrap_or_else(|| Err(unknown()))
    }

    async fn verify_jwt(&self, token: &str) -> Result<Caller, AuthError> {
        let invalid = |e: String| AuthError::Unauthenticated(e);
        let header = decode_header(token).map_err(|e| invalid(e.to_string()))?;
        let key = match (header.alg, &self.hs256, header.kid.as_deref()) {
            (Algorithm::HS256, Some(key), _) => key.clone(),
            (Algorithm::HS256, None, _) => return Err(invalid("HS256 token but no secret configured".into())),
            (_, _, Some(kid)) => self.key_for(kid).await?,
            (alg, _, None) => return Err(invalid(format!("{alg:?} token without a key id"))),
        };
        let mut validation = Validation::new(header.alg);
        validation.set_audience(&[AUDIENCE]);
        let data = decode::<Claims>(token, &key, &validation).map_err(|e| invalid(e.to_string()))?;
        let claims = data.claims;
        let email = claims
            .email
            .as_deref()
            .map(str::trim)
            .filter(|e| e.contains('@'))
            .ok_or_else(|| invalid("token has no usable email".into()))?;
        if claims.sub.trim().is_empty() {
            return Err(invalid("token has no subject".into()));
        }
        Ok(self.caller(&claims.sub, email, claims.user_metadata.and_then(|m| m.full_name)))
    }
}

impl BillingIdentity for JwtIdentity {
    fn authenticate<'a>(&'a self, headers: &'a HeaderMap) -> BoxFuture<'a, Result<Caller, AuthError>> {
        Box::pin(async move {
            let token = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| AuthError::Unauthenticated("no bearer token".into()))?;
            if let Some(email) = token.strip_prefix(DEV_PREFIX) {
                if !self.dev_tokens {
                    return Err(AuthError::Unauthenticated("development tokens are not enabled".into()));
                }
                if !email.contains('@') {
                    return Err(AuthError::Unauthenticated("a development token is dev:<email>".into()));
                }
                return Ok(self.caller(&format!("dev-{email}"), email, None));
            }
            self.verify_jwt(token).await
        })
    }
}

/// The configured JWKS path against a local endpoint whose keys rotate, with time
/// moved by hand. The RSA keys are synthetic (`tests/fixtures/README.md`).
#[cfg(test)]
mod jwks_tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderValue, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum::{Json, Router};
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    const N_A: &str = "02I5B24dVVbqxQVjIlqQxhq4kHf5XzIDSsRm-o5fv4VjTL_WUyflCoUAQa_1HkUMsSmoxOFjm4nzz4BW_gd_7PTdC8pIKxrxPvrjTmwMHlDnZ4W3umvLiL_3v-Vt1FHXXlqXeDEPeQRVPGksCXVncKS9FlAdLNwxt-b18--JLTNd2U_TpOnfzxTjmVvM4IR1nLp7UeKItgM120Ve8hKz6wfWbwWbzZvlo52p4Ab0BBc7HDLhh4yvxjeKndhhkavWqjwk_CvwFLTePv3cGbB85yHvCsLaRScQ3HaXsub3fpl9Nf0E4kfRe1bbnOPWrZMy1VY5gG7LDsOcXSScn7x1Qw";
    const N_B: &str = "vDKAAGcFrTSEscrdToBly5ZpR33CjM_iw4pcg_p3XSACzIa-fbOVmBKkmnWTqUsZrikbjXdlB4Z7faHnR0XBwcskRYF4CUhnDHpoqixaaYuopRKwOzp2ywxxX8ou07dNQxcIBfxTwcco9Bev1HPCw0Ch-8j5y0LJXwB1E1yyUitNOUTY_iHVCDFduo3etUTEs_YQRq-JKNgOruDqPxfBWcAYbkgcGXTZOwyyioMFnSIBjTHFkzsNOjBG4SyUIQZdezdjoqGkVuh5auAY3dWmFLUWnxqLaDDwMWJJGj2AZ1cK-DkBOafUwcGshoM0Qp4z1fbIcZY0P1OyHb0gD0KnPQ";
    const A: (&str, &str) = ("key-a", N_A);
    const B: (&str, &str) = ("key-b", N_B);
    const PEM_A: &str = include_str!("../tests/fixtures/jwks-test-key-a.pem");
    const PEM_B: &str = include_str!("../tests/fixtures/jwks-test-key-b.pem");

    #[derive(Default)]
    struct Provider {
        keys: Vec<Value>,
        down: bool,
        fetches: usize,
    }

    #[derive(Clone, Default)]
    struct Jwks(Arc<Mutex<Provider>>);

    impl Jwks {
        fn list(&self, keys: &[(&str, &str)]) {
            self.0.lock().unwrap().keys = keys
                .iter()
                .map(|(kid, n)| json!({"kty": "RSA", "alg": "RS256", "use": "sig", "kid": kid, "n": n, "e": "AQAB"}))
                .collect();
        }

        fn down(&self, down: bool) {
            self.0.lock().unwrap().down = down;
        }

        fn fetches(&self) -> usize {
            self.0.lock().unwrap().fetches
        }
    }

    async fn serve(State(jwks): State<Jwks>) -> Response {
        let mut provider = jwks.0.lock().unwrap();
        provider.fetches += 1;
        if provider.down {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        Json(json!({"keys": provider.keys})).into_response()
    }

    struct Clock(Arc<Mutex<Instant>>);

    impl Clock {
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    /// A provider listing `keys`, and the verifier configured with its JWKS URL.
    async fn provider(keys: &[(&str, &str)]) -> (Jwks, JwtIdentity, Clock) {
        let jwks = Jwks::default();
        jwks.list(keys);
        let router = Router::new().route("/jwks.json", get(serve)).with_state(jwks.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/jwks.json", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let clock = Clock(Arc::new(Mutex::new(Instant::now())));
        let mut identity = JwtIdentity::new(None, Some(url), false, BTreeSet::new()).unwrap();
        let at = clock.0.clone();
        identity.now = Box::new(move || *at.lock().unwrap());
        (jwks, identity, clock)
    }

    fn rs256(kid: &str, pem: &str) -> String {
        let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600;
        let claims = json!({"sub": "11111111-2222-4333-8444-555555555555", "email": "ada@example.test", "aud": AUDIENCE, "exp": exp});
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.into());
        encode(&header, &claims, &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap()).unwrap()
    }

    async fn check(identity: &JwtIdentity, token: &str) -> Result<(), AuthError> {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
        identity.authenticate(&headers).await.map(|caller| assert_eq!(caller.email, "ada@example.test"))
    }

    #[tokio::test]
    async fn a_cached_key_is_trusted_only_for_the_cache_lifetime_and_a_removed_key_then_stops_verifying() {
        let (jwks, identity, clock) = provider(&[A]).await;
        let a = rs256(A.0, PEM_A);
        check(&identity, &a).await.unwrap();
        // The provider stops listing the key; inside the lifetime the cache answers alone.
        jwks.list(&[]);
        clock.advance(JWKS_MAX_AGE - Duration::from_secs(1));
        check(&identity, &a).await.unwrap();
        assert_eq!(jwks.fetches(), 1);
        // At the lifetime the set is fetched again and the removed key is refused.
        clock.advance(Duration::from_secs(1));
        assert!(matches!(check(&identity, &a).await, Err(AuthError::Unauthenticated(_))));
        assert_eq!(jwks.fetches(), 2);
        assert!(matches!(check(&identity, &a).await, Err(AuthError::Unauthenticated(_))));
        assert_eq!(jwks.fetches(), 2);
    }

    #[tokio::test]
    async fn a_rotated_in_key_refreshes_the_set_and_the_retired_key_is_refused_at_once() {
        let (jwks, identity, clock) = provider(&[A]).await;
        let (a, b) = (rs256(A.0, PEM_A), rs256(B.0, PEM_B));
        check(&identity, &a).await.unwrap();
        jwks.list(&[B]);
        clock.advance(JWKS_REFRESH_AT_MOST_EVERY);
        check(&identity, &b).await.unwrap();
        assert_eq!(jwks.fetches(), 2);
        // Long before the first set's lifetime would have ended.
        assert!(matches!(check(&identity, &a).await, Err(AuthError::Unauthenticated(_))));
        assert_eq!(jwks.fetches(), 2);
    }

    #[tokio::test]
    async fn an_expired_set_that_cannot_be_refreshed_fails_closed_until_the_provider_answers() {
        let (jwks, identity, clock) = provider(&[A]).await;
        let a = rs256(A.0, PEM_A);
        check(&identity, &a).await.unwrap();
        jwks.down(true);
        clock.advance(JWKS_MAX_AGE);
        assert!(matches!(check(&identity, &a).await, Err(AuthError::Unavailable(_))));
        // The failure does not reinstate the stale set: the next request asks again.
        assert!(matches!(check(&identity, &a).await, Err(AuthError::Unavailable(_))));
        assert_eq!(jwks.fetches(), 3);
        jwks.down(false);
        check(&identity, &a).await.unwrap();
        assert_eq!(jwks.fetches(), 4);
    }

    #[tokio::test]
    async fn unknown_key_ids_refresh_the_set_at_most_once_per_throttle_window() {
        let (jwks, identity, clock) = provider(&[A]).await;
        check(&identity, &rs256(A.0, PEM_A)).await.unwrap();
        let forged = rs256("not-listed", PEM_B);
        for _ in 0..5 {
            assert!(matches!(check(&identity, &forged).await, Err(AuthError::Unauthenticated(_))));
        }
        clock.advance(JWKS_REFRESH_AT_MOST_EVERY - Duration::from_secs(1));
        assert!(matches!(check(&identity, &forged).await, Err(AuthError::Unauthenticated(_))));
        assert_eq!(jwks.fetches(), 1);
        clock.advance(Duration::from_secs(1));
        for _ in 0..5 {
            assert!(matches!(check(&identity, &forged).await, Err(AuthError::Unauthenticated(_))));
        }
        assert_eq!(jwks.fetches(), 2);
    }

    #[tokio::test]
    async fn concurrent_requests_on_an_expired_set_share_one_refresh() {
        let (jwks, identity, clock) = provider(&[A]).await;
        let a = rs256(A.0, PEM_A);
        check(&identity, &a).await.unwrap();
        clock.advance(JWKS_MAX_AGE);
        let (r1, r2, r3, r4) =
            tokio::join!(check(&identity, &a), check(&identity, &a), check(&identity, &a), check(&identity, &a));
        for r in [r1, r2, r3, r4] {
            r.unwrap();
        }
        assert_eq!(jwks.fetches(), 2);
    }
}
