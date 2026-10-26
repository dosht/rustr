//! `Identity` adapters: Supabase Auth JWTs (HS256 shared secret or the
//! project's JWKS), and opt-in dev tokens for local work and tests.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use taskr_core::shared::access::{Email, Identity, IdentityError, Principal, UserId};
use taskr_core::shared::future::BoxFuture;
use tokio::sync::RwLock;

const AUDIENCE: &str = "authenticated";
/// A fetched key set is trusted for at most this long; after it the set is
/// fetched again, and a key the provider no longer lists stops verifying.
const JWKS_MAX_AGE: Duration = Duration::from_secs(600);
/// An unknown key id triggers a refresh at most this often.
const JWKS_REFRESH_AT_MOST_EVERY: Duration = Duration::from_secs(60);
/// A refresh holds the cache lock, so a provider that hangs fails instead.
const JWKS_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct Claims {
    sub: String,
    email: Option<String>,
}

fn principal(claims: Claims) -> Result<Principal, IdentityError> {
    let email = claims
        .email
        .as_deref()
        .and_then(|e| Email::parse(e).ok())
        .ok_or_else(|| IdentityError::Invalid("token has no usable email".into()))?;
    Ok(Principal { user_id: UserId(claims.sub), email })
}

fn validation(alg: Algorithm) -> Validation {
    let mut v = Validation::new(alg);
    v.set_audience(&[AUDIENCE]);
    v
}

/// Verifies Supabase access tokens. Projects on the legacy shared secret use
/// `hs256_secret`; projects on asymmetric signing keys use `jwks_url`
/// (`https://<project>.supabase.co/auth/v1/.well-known/jwks.json`).
pub struct SupabaseJwt {
    hs256: Option<DecodingKey>,
    jwks_url: Option<String>,
    /// The last fetched key set and when it was fetched.
    jwks: RwLock<Option<(JwkSet, Instant)>>,
    http: reqwest::Client,
    /// `Instant::now`, replaced by tests to move time deterministically.
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl SupabaseJwt {
    pub fn new(hs256_secret: Option<&str>, jwks_url: Option<String>) -> Self {
        Self {
            hs256: hs256_secret.map(|s| DecodingKey::from_secret(s.as_bytes())),
            jwks_url,
            jwks: RwLock::new(None),
            // Same failure mode as `reqwest::Client::new`: no TLS backend.
            http: reqwest::Client::builder()
                .timeout(JWKS_FETCH_TIMEOUT)
                .build()
                .expect("HTTP client for the JWKS endpoint"),
            now: Box::new(Instant::now),
        }
    }

    fn age(&self, fetched_at: Instant) -> Duration {
        (self.now)().saturating_duration_since(fetched_at)
    }

    async fn key_for(&self, kid: &str) -> Result<DecodingKey, IdentityError> {
        let url = self.jwks_url.as_deref().ok_or_else(|| {
            IdentityError::Invalid("asymmetric token but no JWKS configured".into())
        })?;
        let decoding = |jwk: &Jwk| DecodingKey::from_jwk(jwk).map_err(|e| IdentityError::Invalid(e.to_string()));
        let unknown = || IdentityError::Invalid(format!("unknown signing key {kid}"));
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
            .map_err(|e| IdentityError::Unavailable(format!("JWKS: {e}")))?
            .json()
            .await
            .map_err(|e| IdentityError::Unavailable(format!("JWKS: {e}")))?;
        let found = set.find(kid).map(decoding);
        *cache = Some((set, (self.now)()));
        found.unwrap_or_else(|| Err(unknown()))
    }

    async fn verify_token(&self, token: &str) -> Result<Principal, IdentityError> {
        let header = decode_header(token).map_err(|e| IdentityError::Invalid(e.to_string()))?;
        let key = match (header.alg, &self.hs256, header.kid.as_deref()) {
            (Algorithm::HS256, Some(key), _) => key.clone(),
            (Algorithm::HS256, None, _) => {
                return Err(IdentityError::Invalid("HS256 token but no secret configured".into()));
            }
            (_, _, Some(kid)) => self.key_for(kid).await?,
            (alg, _, None) => {
                return Err(IdentityError::Invalid(format!("{alg:?} token without a key id")));
            }
        };
        let data = decode::<Claims>(token, &key, &validation(header.alg))
            .map_err(|e| IdentityError::Invalid(e.to_string()))?;
        principal(data.claims)
    }
}

impl Identity for SupabaseJwt {
    fn verify<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Principal, IdentityError>> {
        Box::pin(self.verify_token(token))
    }
}

/// Accepts `dev:<email>` as that person. Never enable it on a reachable server.
pub struct DevIdentity;

pub const DEV_PREFIX: &str = "dev:";

impl Identity for DevIdentity {
    fn verify<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Principal, IdentityError>> {
        Box::pin(async move {
            let email = token
                .strip_prefix(DEV_PREFIX)
                .and_then(|e| Email::parse(e).ok())
                .ok_or_else(|| IdentityError::Invalid("not a dev token".into()))?;
            Ok(Principal { user_id: UserId(format!("dev-{email}")), email })
        })
    }
}

/// Dev tokens go to the dev identity when it is enabled, everything else to
/// the real provider.
pub struct Identities {
    pub dev: Option<Arc<dyn Identity>>,
    pub real: Option<Arc<dyn Identity>>,
}

impl Identity for Identities {
    fn verify<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Principal, IdentityError>> {
        Box::pin(async move {
            let chosen = match (&self.dev, &self.real) {
                (Some(dev), _) if token.starts_with(DEV_PREFIX) => dev,
                (_, Some(real)) => real,
                _ => {
                    return Err(IdentityError::Invalid(
                        "no identity provider accepts this token".into(),
                    ));
                }
            };
            chosen.verify(token).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde::Serialize;
    use taskr_core::testing::block_on;

    #[derive(Serialize)]
    struct TestClaims<'a> {
        sub: &'a str,
        email: &'a str,
        aud: &'a str,
        exp: u64,
    }

    fn token(secret: &str, aud: &str, exp: u64) -> String {
        let claims = TestClaims { sub: "u-123", email: "Ada@Example.com", aud, exp };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    const FAR_FUTURE: u64 = 4_000_000_000;

    fn verify(id: &dyn Identity, t: &str) -> Result<Principal, IdentityError> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _g = rt.enter();
        block_on(id.verify(t))
    }

    #[test]
    fn a_valid_supabase_token_becomes_a_principal() {
        let id = SupabaseJwt::new(Some("super-secret-jwt-token-with-at-least-32-characters"), None);
        let p = verify(
            &id,
            &token("super-secret-jwt-token-with-at-least-32-characters", AUDIENCE, FAR_FUTURE),
        )
        .unwrap();
        assert_eq!(p.user_id, UserId("u-123".into()));
        assert_eq!(p.email.as_str(), "ada@example.com");
    }

    #[test]
    fn wrong_secret_wrong_audience_and_expired_tokens_are_refused() {
        let secret = "super-secret-jwt-token-with-at-least-32-characters";
        let id = SupabaseJwt::new(Some(secret), None);
        assert!(
            verify(&id, &token("another-secret-another-secret-another", AUDIENCE, FAR_FUTURE))
                .is_err()
        );
        assert!(verify(&id, &token(secret, "anon", FAR_FUTURE)).is_err());
        assert!(verify(&id, &token(secret, AUDIENCE, 1_000)).is_err());
        assert!(verify(&id, "not.a.jwt").is_err());
    }

    #[test]
    fn dev_tokens_only_work_when_dev_is_enabled() {
        let real: Arc<dyn Identity> =
            Arc::new(SupabaseJwt::new(Some("x".repeat(32).as_str()), None));
        let with_dev = Identities { dev: Some(Arc::new(DevIdentity)), real: Some(real.clone()) };
        let without = Identities { dev: None, real: Some(real) };
        assert_eq!(
            verify(&with_dev, "dev:bob@example.com").unwrap().email.as_str(),
            "bob@example.com"
        );
        assert!(verify(&without, "dev:bob@example.com").is_err());
    }

    /// The configured JWKS path against a local endpoint whose keys rotate, with
    /// time moved by hand. The RSA keys are synthetic (`tests/fixtures/README.md`).
    mod jwks {
        use super::*;
        use axum::extract::State;
        use axum::http::StatusCode;
        use axum::response::{IntoResponse, Response};
        use axum::routing::get;
        use axum::{Json, Router};
        use serde_json::{Value, json};
        use std::sync::Mutex;
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
        async fn provider(keys: &[(&str, &str)]) -> (Jwks, SupabaseJwt, Clock) {
            let jwks = Jwks::default();
            jwks.list(keys);
            let router = Router::new().route("/jwks.json", get(serve)).with_state(jwks.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/jwks.json", listener.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let clock = Clock(Arc::new(Mutex::new(Instant::now())));
            let mut identity = SupabaseJwt::new(None, Some(url));
            let at = clock.0.clone();
            identity.now = Box::new(move || *at.lock().unwrap());
            (jwks, identity, clock)
        }

        fn rs256(kid: &str, pem: &str) -> String {
            let exp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 3600;
            let claims = TestClaims { sub: "u-123", email: "Ada@Example.com", aud: AUDIENCE, exp };
            let mut header = Header::new(Algorithm::RS256);
            header.kid = Some(kid.into());
            encode(&header, &claims, &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap()).unwrap()
        }

        async fn check(identity: &SupabaseJwt, token: &str) -> Result<(), IdentityError> {
            identity.verify(token).await.map(|p| assert_eq!(p.user_id, UserId("u-123".into())))
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
            assert!(matches!(check(&identity, &a).await, Err(IdentityError::Invalid(_))));
            assert_eq!(jwks.fetches(), 2);
            assert!(matches!(check(&identity, &a).await, Err(IdentityError::Invalid(_))));
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
            assert!(matches!(check(&identity, &a).await, Err(IdentityError::Invalid(_))));
            assert_eq!(jwks.fetches(), 2);
        }

        #[tokio::test]
        async fn an_expired_set_that_cannot_be_refreshed_fails_closed_until_the_provider_answers() {
            let (jwks, identity, clock) = provider(&[A]).await;
            let a = rs256(A.0, PEM_A);
            check(&identity, &a).await.unwrap();
            jwks.down(true);
            clock.advance(JWKS_MAX_AGE);
            assert!(matches!(check(&identity, &a).await, Err(IdentityError::Unavailable(_))));
            // The failure does not reinstate the stale set: the next request asks again.
            assert!(matches!(check(&identity, &a).await, Err(IdentityError::Unavailable(_))));
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
                assert!(matches!(check(&identity, &forged).await, Err(IdentityError::Invalid(_))));
            }
            clock.advance(JWKS_REFRESH_AT_MOST_EVERY - Duration::from_secs(1));
            assert!(matches!(check(&identity, &forged).await, Err(IdentityError::Invalid(_))));
            assert_eq!(jwks.fetches(), 1);
            clock.advance(Duration::from_secs(1));
            for _ in 0..5 {
                assert!(matches!(check(&identity, &forged).await, Err(IdentityError::Invalid(_))));
            }
            assert_eq!(jwks.fetches(), 2);
        }

        #[tokio::test]
        async fn concurrent_requests_on_an_expired_set_share_one_refresh() {
            let (jwks, identity, clock) = provider(&[A]).await;
            let a = rs256(A.0, PEM_A);
            check(&identity, &a).await.unwrap();
            clock.advance(JWKS_MAX_AGE);
            let (r1, r2, r3, r4) = tokio::join!(
                check(&identity, &a),
                check(&identity, &a),
                check(&identity, &a),
                check(&identity, &a)
            );
            for r in [r1, r2, r3, r4] {
                r.unwrap();
            }
            assert_eq!(jwks.fetches(), 2);
        }
    }
}
