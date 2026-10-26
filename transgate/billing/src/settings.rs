//! Configuration from the environment, parsed through a lookup function so it
//! is testable without touching the process environment.
//!
//! The rule: billing is either fully configured or refused loudly. A group of
//! settings that is half present is an error naming what is missing. Nothing
//! here ever selects a fake adapter.

use std::collections::BTreeSet;

use billing_core::decimal::Decimal;
use billing_core::pricing::{SpecialClientOffer, parse_rate};
use billing_core::types::AccountId;

pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

#[derive(Clone, Debug)]
pub struct IyzicoSettings {
    pub base_url: String,
    pub api_key: String,
    pub secret_key: String,
    pub merchant_id: String,
    pub environment: String,
}

#[derive(Clone, Debug)]
pub struct LemonSettings {
    pub base_url: String,
    pub api_key: String,
    pub store_id: String,
    pub variant_id: String,
    pub signing_secret: String,
    pub environment: String,
}

#[derive(Clone, Debug)]
pub struct MailSettings {
    pub url: String,
    pub api_key: String,
    pub from: String,
    pub sales_email: String,
    pub support_email: String,
}

/// PostHog server-side analytics: the source's `POSTHOG_HOST` and `POSTHOG_API_KEY`.
#[derive(Clone, Debug)]
pub struct AnalyticsSettings {
    pub host: String,
    pub api_key: String,
}

/// The trusted auth directory that supplies real signup times and verified contact facts.
#[derive(Clone, Debug)]
pub struct AuthDirectorySettings {
    pub url: String,
    pub service_key: String,
}
#[derive(Clone, Debug)]
pub struct IdentitySettings {
    pub hs256_secret: Option<String>,
    pub jwks_url: Option<String>,
    pub dev_tokens: bool,
    pub admin_emails: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Eligibility {
    pub special_offer_emails: BTreeSet<String>,
    pub special_clients: BTreeSet<AccountId>,
    pub special_client_offer: Option<SpecialClientOffer>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    /// The restricted runtime login. Never an owner connection.
    pub database_url: String,
    /// Owner authority, used only to migrate at startup when given.
    pub migration_database_url: Option<String>,
    /// Where providers send the browser back: configured, not read from Host.
    pub public_url: String,
    pub rate: Decimal,
    pub iyzico: Option<IyzicoSettings>,
    pub lemon: Option<LemonSettings>,
    pub identity: IdentitySettings,
    pub mail: MailSettings,
    pub analytics: Option<AnalyticsSettings>,
    pub auth_directory: Option<AuthDirectorySettings>,
    pub signup_token: Option<String>,
    pub eligibility: Eligibility,
}

fn list(value: Option<String>) -> Vec<String> {
    value
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// All of a group or none of it.
fn group(get: Lookup<'_>, names: &[&str], label: &str) -> Result<Option<Vec<String>>, String> {
    let values: Vec<Option<String>> = names.iter().map(|n| get(n)).collect();
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let missing: Vec<&str> = names.iter().zip(&values).filter(|(_, v)| v.is_none()).map(|(n, _)| *n).collect();
    if !missing.is_empty() {
        return Err(format!("{label} is partly configured; also set {}", missing.join(", ")));
    }
    Ok(Some(values.into_iter().map(|v| v.unwrap_or_default()).collect()))
}

fn environment(value: &str, allowed: &[&str], name: &str) -> Result<String, String> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!("{name} must be one of {}", allowed.join(", ")))
    }
}

/// `Ok(None)` means billing is not configured at all (no database): the host
/// stays process-liveness only and says so. Anything else is complete or an error.
pub fn settings_from(get: Lookup<'_>) -> Result<Option<Settings>, String> {
    let Some(database_url) = get("TRANSGATE_DATABASE_URL") else { return Ok(None) };
    let public_url = get("TRANSGATE_PUBLIC_URL")
        .ok_or("TRANSGATE_PUBLIC_URL is required: the address providers send customers back to")?;

    let iyzico = group(
        get,
        &["IYZIPAY_URI", "IYZIPAY_API_KEY", "IYZIPAY_SECRET_KEY", "IYZIPAY_MERCHANT_ID", "IYZIPAY_ENVIRONMENT"],
        "Iyzico",
    )?
    .map(|v| -> Result<IyzicoSettings, String> {
        Ok(IyzicoSettings {
            base_url: v[0].clone(),
            api_key: v[1].clone(),
            secret_key: v[2].clone(),
            merchant_id: v[3].clone(),
            environment: environment(&v[4], &["sandbox", "live"], "IYZIPAY_ENVIRONMENT")?,
        })
    })
    .transpose()?;

    let lemon = group(
        get,
        &[
            "LEMONSQUEEZY_API_KEY",
            "LEMONSQUEEZY_STORE_ID",
            "LEMONSQUEEZY_VARIANT_ID",
            "LEMONSQUEEZY_WEBHOOK_SECRET",
            "LEMONSQUEEZY_ENVIRONMENT",
        ],
        "LemonSqueezy",
    )?
    .map(|v| -> Result<LemonSettings, String> {
        Ok(LemonSettings {
            base_url: get("LEMONSQUEEZY_API_URL").unwrap_or_else(|| "https://api.lemonsqueezy.com".into()),
            api_key: v[0].clone(),
            store_id: v[1].clone(),
            variant_id: v[2].clone(),
            signing_secret: v[3].clone(),
            environment: environment(&v[4], &["test", "live"], "LEMONSQUEEZY_ENVIRONMENT")?,
        })
    })
    .transpose()?;

    if iyzico.is_none() && lemon.is_none() {
        return Err("no payment gateway is configured: set the IYZIPAY_* or the LEMONSQUEEZY_* group".into());
    }

    let identity = IdentitySettings {
        hs256_secret: get("SUPABASE_JWT_SECRET"),
        jwks_url: get("SUPABASE_JWKS_URL"),
        dev_tokens: get("TRANSGATE_DEV_AUTH").as_deref() == Some("1"),
        admin_emails: list(get("TRANSGATE_ADMIN_EMAILS")).into_iter().map(|e| e.to_lowercase()).collect(),
    };
    if identity.hs256_secret.is_none() && identity.jwks_url.is_none() && !identity.dev_tokens {
        return Err("no identity is configured: set SUPABASE_JWT_SECRET or SUPABASE_JWKS_URL (or TRANSGATE_DEV_AUTH=1 locally)".into());
    }

    // Billing promises purchase, sales and cancellation emails, so delivery is part of startup.
    let mail = group(
        get,
        &["TRANSGATE_MAIL_URL", "TRANSGATE_MAIL_API_KEY", "TRANSGATE_MAIL_FROM", "TRANSGATE_SALES_EMAIL", "TRANSGATE_SUPPORT_EMAIL"],
        "Mail delivery",
    )?
    .map(|v| MailSettings {
        url: v[0].clone(),
        api_key: v[1].clone(),
        from: v[2].clone(),
        sales_email: v[3].clone(),
        support_email: v[4].clone(),
    })
    .ok_or("mail delivery is required: set TRANSGATE_MAIL_URL, TRANSGATE_MAIL_API_KEY, TRANSGATE_MAIL_FROM, TRANSGATE_SALES_EMAIL and TRANSGATE_SUPPORT_EMAIL")?;

    let analytics = group(get, &["POSTHOG_HOST", "POSTHOG_API_KEY"], "PostHog analytics")?
        .map(|v| AnalyticsSettings { host: v[0].clone(), api_key: v[1].clone() });

    let auth_directory = group(get, &["SUPABASE_URL", "SUPABASE_SERVICE_ROLE_KEY"], "The Supabase auth directory")?
        .map(|v| AuthDirectorySettings { url: v[0].clone(), service_key: v[1].clone() });
    let signup_token = get("TRANSGATE_SIGNUP_TOKEN");
    // An authenticated account is provisioned only from a trusted record of its real signup time
    // and verified contact facts. Production composition therefore needs the auth directory to
    // ask; the service-token route is only an additional way for the host's signup system to
    // report, never a substitute for the producer. Local development with development tokens
    // may run without it.
    if auth_directory.is_none() && !identity.dev_tokens {
        return Err("no trusted signup source is configured: set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY (TRANSGATE_SIGNUP_TOKEN alone is only an extra reporting route)".into());
    }

    let mut eligibility = Eligibility {
        special_offer_emails: list(get("TRANSGATE_SPECIAL_OFFER_EMAILS")).into_iter().map(|e| e.to_lowercase()).collect(),
        ..Eligibility::default()
    };
    for id in list(get("TRANSGATE_SPECIAL_CLIENTS")) {
        let account = AccountId::parse(&id).ok_or_else(|| format!("TRANSGATE_SPECIAL_CLIENTS holds {id:?}, which is not a UUID"))?;
        eligibility.special_clients.insert(account);
    }
    if let Some(offer) = get("TRANSGATE_SPECIAL_CLIENT_OFFER") {
        let (code, percent) = offer
            .rsplit_once(':')
            .ok_or("TRANSGATE_SPECIAL_CLIENT_OFFER is CODE:PERCENT")?;
        let percent: u32 = percent.trim().parse().map_err(|_| "the offer percentage must be a whole number")?;
        let message = get("TRANSGATE_SPECIAL_CLIENT_MESSAGE")
            .ok_or("TRANSGATE_SPECIAL_CLIENT_MESSAGE is required with TRANSGATE_SPECIAL_CLIENT_OFFER: the alert text is host data")?;
        eligibility.special_client_offer = Some(SpecialClientOffer { code: code.trim().to_string(), percent, message });
    }

    Ok(Some(Settings {
        database_url,
        migration_database_url: get("TRANSGATE_MIGRATION_DATABASE_URL"),
        public_url,
        rate: parse_rate(get("USD_TRY_RATE").as_deref()),
        iyzico,
        lemon,
        identity,
        mail,
        analytics,
        auth_directory,
        signup_token,
        eligibility,
    }))
}
