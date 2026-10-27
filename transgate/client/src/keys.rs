//! The key screen's session routes (`/api/keys`, WEB-045/046), typed with the wire
//! shape `transgate_api::public` writes (camelCase, ISO-8601 instants) and mapped
//! into core's `KeyView`. Nothing here decides what an owner may see or do: scope
//! checks, limits and the shown-once rule are core's.
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use transgate_core::Micros;
use transgate_core::api::keys::{CreatedKey, ExpirationPolicy, KeyView, NewKey};
use transgate_core::api::KeyId;
use transgate_core::jobs::correlate::parse_rfc3339;
use uuid::Uuid;

use crate::api::{ApiError, HttpApi};

/// `ApiKeyView` of `transgate_api::public`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyWire {
    id: Uuid,
    name: String,
    created_at: String,
    expires_at: Option<String>,
    is_active: bool,
    scopes: Vec<String>,
    expiration_policy: ExpirationPolicy,
    hint: Option<String>,
}

fn instant(text: &str) -> Result<Micros, ApiError> {
    parse_rfc3339(text).ok_or_else(|| ApiError::Decode(format!("bad timestamp {text}")))
}

impl KeyWire {
    fn into_view(self) -> Result<KeyView, ApiError> {
        Ok(KeyView {
            id: KeyId(self.id),
            name: self.name,
            created_at: instant(&self.created_at)?,
            expires_at: self.expires_at.as_deref().map(instant).transpose()?,
            active: self.is_active,
            scopes: self.scopes,
            expiration_policy: self.expiration_policy,
            hint: self.hint,
        })
    }
}

#[derive(Deserialize)]
struct ListWire {
    keys: Vec<KeyWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedWire {
    key: KeyWire,
    api_key: String,
}

#[derive(Deserialize)]
struct RevokedWire {
    key: KeyWire,
}

#[derive(Deserialize)]
struct SecretWire {
    secret: String,
}

impl HttpApi {
    /// `GET /api/keys`.
    pub async fn list_keys(&self, user: Uuid) -> Result<Vec<KeyView>, ApiError> {
        let wire: ListWire = self.json(user, Method::GET, "/keys", None).await?;
        wire.keys.into_iter().map(KeyWire::into_view).collect()
    }

    /// `GET /api/keys/signing-secret`: the owner's callback signing secret.
    pub async fn signing_secret(&self, user: Uuid) -> Result<String, ApiError> {
        let wire: SecretWire = self.json(user, Method::GET, "/keys/signing-secret", None).await?;
        Ok(wire.secret)
    }

    /// `POST /api/keys` (201): the plaintext key comes back exactly once.
    pub async fn create_key(&self, user: Uuid, new: &NewKey) -> Result<CreatedKey, ApiError> {
        let mut body = json!({
            "name": new.name,
            "scopes": serde_json::to_value(&new.scopes).map_err(|e| ApiError::Decode(e.to_string()))?,
            "expirationPolicy": serde_json::to_value(new.expiration_policy).map_err(|e| ApiError::Decode(e.to_string()))?,
        });
        if let Some(days) = new.custom_days {
            body["customDays"] = Value::from(days);
        }
        let wire: CreatedWire = self.json(user, Method::POST, "/keys", Some(&body)).await?;
        Ok(CreatedKey { key: wire.key.into_view()?, api_key: wire.api_key })
    }

    /// `DELETE /api/keys/{id}`: the key as it is after revocation.
    pub async fn revoke_key(&self, user: Uuid, id: KeyId) -> Result<KeyView, ApiError> {
        let wire: RevokedWire = self.json(user, Method::DELETE, &format!("/keys/{}", id.0), None).await?;
        wire.key.into_view()
    }
}
