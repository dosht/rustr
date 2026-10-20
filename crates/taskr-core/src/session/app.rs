use crate::shared::clock::Millis;
use crate::shared::future::BoxFuture;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub email: String,
    pub expires_at: Option<Millis>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthError {
    InvalidCredentials,
    AlreadyRegistered,
    /// The provider refused for a reason it explained (weak password, rate limit).
    Rejected(String),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignUp {
    SignedIn(Session),
    /// The provider sent a confirmation email; sign in after confirming.
    ConfirmationSent,
}

/// Client-side port to the identity provider (Supabase Auth, or dev tokens).
pub trait AuthClient: Send + Sync {
    fn sign_in<'a>(
        &'a self,
        email: &'a str,
        password: &'a str,
    ) -> BoxFuture<'a, Result<Session, AuthError>>;

    fn sign_up<'a>(
        &'a self,
        email: &'a str,
        password: &'a str,
    ) -> BoxFuture<'a, Result<SignUp, AuthError>>;
}

/// Where a shell keeps the session between launches (localStorage, a file,
/// the keychain). Synchronous because every platform store is.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Option<Session>;
    fn save(&self, session: &Session);
    fn clear(&self);
}

pub fn describe_auth(e: &AuthError) -> String {
    match e {
        AuthError::InvalidCredentials => "Wrong email or password.".into(),
        AuthError::AlreadyRegistered => {
            "That email already has an account. Sign in instead.".into()
        }
        AuthError::Rejected(why) => why.clone(),
        AuthError::Unavailable(why) => format!("Could not reach the sign-in service ({why})."),
    }
}
