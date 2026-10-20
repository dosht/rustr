//! `Identity` adapters: Supabase Auth JWTs (HS256 shared secret or the
//! project's JWKS), and opt-in dev tokens for local work and tests.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use taskr_core::shared::access::{Email, Identity, IdentityError, Principal, UserId};
use taskr_core::shared::future::BoxFuture;
use tokio::sync::RwLock;

const AUDIENCE: &str = "authenticated";
const JWKS_REFRESH_AT_MOST_EVERY: Duration = Duration::from_secs(60);

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
    jwks: RwLock<(Option<JwkSet>, Option<Instant>)>,
    http: reqwest::Client,
}

impl SupabaseJwt {
    pub fn new(hs256_secret: Option<&str>, jwks_url: Option<String>) -> Self {
        Self {
            hs256: hs256_secret.map(|s| DecodingKey::from_secret(s.as_bytes())),
            jwks_url,
            jwks: RwLock::new((None, None)),
            http: reqwest::Client::new(),
        }
    }

    async fn key_for(&self, kid: &str) -> Result<DecodingKey, IdentityError> {
        if let Some(jwk) = self.jwks.read().await.0.as_ref().and_then(|s| s.find(kid)) {
            return DecodingKey::from_jwk(jwk).map_err(|e| IdentityError::Invalid(e.to_string()));
        }
        let url = self.jwks_url.as_deref().ok_or_else(|| {
            IdentityError::Invalid("asymmetric token but no JWKS configured".into())
        })?;
        let mut cache = self.jwks.write().await;
        let recently = cache.1.is_some_and(|at| at.elapsed() < JWKS_REFRESH_AT_MOST_EVERY);
        if !recently {
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
            *cache = (Some(set), Some(Instant::now()));
        }
        let jwk = cache
            .0
            .as_ref()
            .and_then(|s| s.find(kid))
            .ok_or_else(|| IdentityError::Invalid(format!("unknown signing key {kid}")))?;
        DecodingKey::from_jwk(jwk).map_err(|e| IdentityError::Invalid(e.to_string()))
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
}
