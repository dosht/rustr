//! `AuthClient` adapters. `SupabaseAuth` talks to Supabase Auth (GoTrue) with
//! the password grant; `DevAuth` mints `dev:<email>` tokens that only a server
//! started with dev auth accepts.

use serde::Deserialize;
use taskr_core::session::app::{AuthClient, AuthError, Session, SignUp};
use taskr_core::shared::access::Email;
use taskr_core::shared::future::BoxFuture;

pub struct SupabaseAuth {
    http: reqwest::Client,
    url: String,
    api_key: Option<String>,
}

impl SupabaseAuth {
    /// `url` is the GoTrue root: `https://<project>.supabase.co/auth/v1` on
    /// Supabase, `http://localhost:9999` for a bare GoTrue container.
    /// `api_key` is the project's anon (publishable) key; bare GoTrue needs none.
    pub fn new(url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            url: url.into().trim_end_matches('/').to_string(),
            api_key,
        }
    }

    async fn post(&self, path: &str, email: &str, password: &str) -> Result<String, AuthError> {
        let mut req = self
            .http
            .post(format!("{}{path}", self.url))
            .json(&serde_json::json!({ "email": email, "password": password }));
        if let Some(key) = &self.api_key {
            req = req.header("apikey", key).bearer_auth(key);
        }
        let res = req.send().await.map_err(|e| AuthError::Unavailable(e.to_string()))?;
        let status = res.status();
        let text = res.text().await.map_err(|e| AuthError::Unavailable(e.to_string()))?;
        if status.is_success() {
            return Ok(text);
        }
        if status.is_server_error() {
            return Err(AuthError::Unavailable(format!("{status}")));
        }
        Err(gotrue_error(&text))
    }
}

/// GoTrue has answered errors in two shapes over the years; read both.
#[derive(Deserialize, Default)]
struct GoTrueError {
    error_code: Option<String>,
    error: Option<String>,
    msg: Option<String>,
    error_description: Option<String>,
}

fn gotrue_error(text: &str) -> AuthError {
    let e: GoTrueError = serde_json::from_str(text).unwrap_or_default();
    let code = e.error_code.or(e.error).unwrap_or_default();
    let message = e.msg.or(e.error_description).unwrap_or_else(|| text.to_string());
    match code.as_str() {
        "invalid_credentials" | "invalid_grant" => AuthError::InvalidCredentials,
        "user_already_exists" | "email_exists" => AuthError::AlreadyRegistered,
        "email_not_confirmed" => {
            AuthError::Rejected("Confirm your email first, then sign in.".into())
        }
        _ if message.contains("already registered") => AuthError::AlreadyRegistered,
        _ => AuthError::Rejected(message),
    }
}

#[derive(Deserialize)]
struct GoTrueSession {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    user: GoTrueUser,
}

#[derive(Deserialize)]
struct GoTrueUser {
    email: Option<String>,
}

fn session(s: GoTrueSession, fallback_email: &str) -> Session {
    Session {
        access_token: s.access_token,
        refresh_token: s.refresh_token,
        email: s.user.email.unwrap_or_else(|| fallback_email.to_string()),
        expires_at: s.expires_at.map(|secs| secs * 1000),
    }
}

impl AuthClient for SupabaseAuth {
    fn sign_in<'a>(
        &'a self,
        email: &'a str,
        password: &'a str,
    ) -> BoxFuture<'a, Result<Session, AuthError>> {
        Box::pin(async move {
            let text = self.post("/token?grant_type=password", email, password).await?;
            let s: GoTrueSession = serde_json::from_str(&text)
                .map_err(|e| AuthError::Unavailable(format!("unexpected answer: {e}")))?;
            Ok(session(s, email))
        })
    }

    fn sign_up<'a>(
        &'a self,
        email: &'a str,
        password: &'a str,
    ) -> BoxFuture<'a, Result<SignUp, AuthError>> {
        Box::pin(async move {
            let text = self.post("/signup", email, password).await?;
            Ok(match serde_json::from_str::<GoTrueSession>(&text) {
                Ok(s) => SignUp::SignedIn(session(s, email)),
                Err(_) => SignUp::ConfirmationSent,
            })
        })
    }
}

/// Signs anyone in without a password check. Only for local development and
/// tests, against a server started with dev auth.
pub struct DevAuth;

impl DevAuth {
    pub const TOKEN_PREFIX: &'static str = "dev:";
}

fn dev_session(email: &str) -> Result<Session, AuthError> {
    let email = Email::parse(email)
        .map_err(|_| AuthError::Rejected("That does not look like an email address.".into()))?;
    Ok(Session {
        access_token: format!("{}{email}", DevAuth::TOKEN_PREFIX),
        refresh_token: None,
        email: email.as_str().to_string(),
        expires_at: None,
    })
}

impl AuthClient for DevAuth {
    fn sign_in<'a>(
        &'a self,
        email: &'a str,
        _password: &'a str,
    ) -> BoxFuture<'a, Result<Session, AuthError>> {
        Box::pin(async move { dev_session(email) })
    }

    fn sign_up<'a>(
        &'a self,
        email: &'a str,
        _password: &'a str,
    ) -> BoxFuture<'a, Result<SignUp, AuthError>> {
        Box::pin(async move { dev_session(email).map(SignUp::SignedIn) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_gotrue_error_shapes_are_understood() {
        assert_eq!(
            gotrue_error(
                r#"{"code":400,"error_code":"invalid_credentials","msg":"Invalid login credentials"}"#
            ),
            AuthError::InvalidCredentials
        );
        assert_eq!(
            gotrue_error(
                r#"{"error":"invalid_grant","error_description":"Invalid login credentials"}"#
            ),
            AuthError::InvalidCredentials
        );
        assert_eq!(
            gotrue_error(
                r#"{"code":422,"error_code":"user_already_exists","msg":"User already registered"}"#
            ),
            AuthError::AlreadyRegistered
        );
        assert_eq!(
            gotrue_error(
                r#"{"code":422,"error_code":"weak_password","msg":"Password should be at least 6 characters."}"#
            ),
            AuthError::Rejected("Password should be at least 6 characters.".into())
        );
    }

    #[test]
    fn dev_tokens_carry_the_normalized_email() {
        let s = dev_session(" Ada@Example.com ").unwrap();
        assert_eq!(s.access_token, "dev:ada@example.com");
        assert!(dev_session("nope").is_err());
    }
}
