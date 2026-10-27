//! The public API's operation table (P7 D9). One table drives three things: the OpenAPI 3.1
//! document generated here (schemas come from the `JsonSchema` derives of the very DTOs that build
//! the response bodies), the router `transgate-http` mounts (it looks every operation's handler up by
//! `id`, so an undocumented route cannot be mounted and a documented one cannot be missing), and
//! the examples the agreement tests feed to the real parsers.

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::{Map, Value, json};

use crate::Invalid;
use crate::public::{
    CreatedKeyResponse, ErrorBody, KeyListResponse, NewKeyRequest, ReplayResponse, RevokedKeyResponse, SigningSecretResponse, StartResponse,
    StatusResponse, TranscriptionsRequest, parse_new_key, parse_public_start,
};

pub const API_VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    /// `Authorization: Bearer sk_...` with this scope.
    Key(&'static str),
    /// A signed-in browser session (its bearer token).
    Session,
    /// A session of an administrator.
    Admin,
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

/// Checks that a request body parses; the agreement test runs it over the documented example.
pub type RequestParser = fn(&[u8]) -> Result<(), Invalid>;

pub struct Operation {
    /// The `operationId`, and the key `transgate-http` mounts the handler by.
    pub id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub summary: &'static str,
    pub description: &'static str,
    pub auth: Auth,
    pub query: &'static [Param],
    pub headers: &'static [Param],
    /// The path parameters, in the order they appear in `path`.
    pub path_params: &'static [Param],
    pub request: Option<fn(&mut SchemaGenerator) -> Schema>,
    /// The documented example body; the agreement test parses it with `parse`.
    pub example: Option<&'static str>,
    pub parse: Option<RequestParser>,
    /// For every documented request property, a value that must change what the parser says about
    /// the example. Proves the parser reads the property the schema documents.
    pub probes: &'static [(&'static str, &'static str)],
    pub success: u16,
    pub success_description: &'static str,
    pub response: fn(&mut SchemaGenerator) -> Schema,
    pub errors: &'static [u16],
    /// One `curl` example.
    pub curl: &'static str,
}

fn schema_of<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<T>()
}

fn start_parse(body: &[u8]) -> Result<(), Invalid> {
    parse_public_start(body).map(|_| ())
}

fn key_parse(body: &[u8]) -> Result<(), Invalid> {
    parse_new_key(body).map(|_| ())
}

pub const OPERATIONS: &[Operation] = &[
    Operation {
        id: "startTranscriptions",
        method: "post",
        path: "/api/v1/transcriptions",
        summary: "Start transcriptions",
        description: "Starts draft jobs by id (`jobIds`) or creates and starts jobs from audio URLs (`audioUrls`). \
            Needs the `transcriptions:create` scope. Send an `Idempotency-Key` to make a retry safe: the same key with the same body \
            returns the first answer without charging again for 24 hours; the same key with another body is `409 request_conflict`. \
            A job can carry a `callbackUrl`: when it completes, a signed POST is delivered (see the callback payload in the guide).",
        auth: Auth::Key("transcriptions:create"),
        query: &[],
        headers: &[Param { name: "Idempotency-Key", description: "Optional. 1 to 255 characters. Makes a retried start return the first answer.", required: false }],
        path_params: &[],
        request: Some(schema_of::<TranscriptionsRequest>),
        example: Some(r#"{"audioUrls":[{"audioUrl":"https://example.com/meeting.mp3","callbackUrl":"https://example.com/hook","language":"English"}]}"#),
        parse: Some(start_parse),
        probes: &[
            ("jobIds", "[1]"),
            ("audioUrls", r#"[{"audioUrl":"not a url"}]"#),
            ("locale", "5"),
            ("options", r#""x""#),
        ],
        success: 201,
        success_description: "The jobs were accepted. Acceptance is not completion.",
        response: schema_of::<StartResponse>,
        errors: &[400, 401, 402, 403, 409, 410, 429, 503],
        curl: "curl -X POST https://transgate.ai/api/v1/transcriptions -H 'Authorization: Bearer $TRANSGATE_API_KEY' -H 'Idempotency-Key: order-1001' -H 'Content-Type: application/json' -d '{\"audioUrls\":[{\"audioUrl\":\"https://example.com/meeting.mp3\"}]}'",
    },
    Operation {
        id: "getTranscription",
        method: "get",
        path: "/api/v1/transcriptions",
        summary: "Get a transcription",
        description: "The status of one of your jobs, with its lines and general summary once it is completed. Needs the `transcriptions:read` scope. \
            A job that is not yours is `404`.",
        auth: Auth::Key("transcriptions:read"),
        query: &[Param { name: "jobId", description: "The job id returned when it was started.", required: true }],
        headers: &[],
        path_params: &[],
        request: None,
        example: None,
        parse: None,
        probes: &[],
        success: 200,
        success_description: "The job and, when completed, its result.",
        response: schema_of::<StatusResponse>,
        errors: &[400, 401, 403, 404, 429, 503],
        curl: "curl 'https://transgate.ai/api/v1/transcriptions?jobId=123' -H 'Authorization: Bearer $TRANSGATE_API_KEY'",
    },
    Operation {
        id: "listApiKeys",
        method: "get",
        path: "/api/keys",
        summary: "List your API keys",
        description: "Your keys, newest first. Never includes a key's secret value.",
        auth: Auth::Session,
        query: &[],
        headers: &[],
        path_params: &[],
        request: None,
        example: None,
        parse: None,
        probes: &[],
        success: 200,
        success_description: "Your keys.",
        response: schema_of::<KeyListResponse>,
        errors: &[401, 403, 410, 503],
        curl: "curl https://transgate.ai/api/keys -H 'Authorization: Bearer $SESSION_TOKEN'",
    },
    Operation {
        id: "createApiKey",
        method: "post",
        path: "/api/keys",
        summary: "Create an API key",
        description: "The plaintext key is in the answer exactly once; only its digest is stored.",
        auth: Auth::Session,
        query: &[],
        headers: &[],
        path_params: &[],
        request: Some(schema_of::<NewKeyRequest>),
        example: Some(r#"{"name":"CI","scopes":["transcriptions:read","transcriptions:create"],"expirationPolicy":"90_days"}"#),
        parse: Some(key_parse),
        probes: &[("name", "5"), ("scopes", r#""x""#), ("expirationPolicy", "5"), ("customDays", r#""x""#)],
        success: 201,
        success_description: "The key and its plaintext value.",
        response: schema_of::<CreatedKeyResponse>,
        errors: &[400, 401, 403, 410, 503],
        curl: "curl -X POST https://transgate.ai/api/keys -H 'Authorization: Bearer $SESSION_TOKEN' -H 'Content-Type: application/json' -d '{\"name\":\"CI\",\"scopes\":[\"transcriptions:read\"],\"expirationPolicy\":\"never\"}'",
    },
    Operation {
        id: "revokeApiKey",
        method: "delete",
        path: "/api/keys/{id}",
        summary: "Revoke an API key",
        description: "Deactivates one of your keys. Another account's key is `404`.",
        auth: Auth::Session,
        query: &[],
        headers: &[],
        path_params: &[Param { name: "id", description: "The key id.", required: true }],
        request: None,
        example: None,
        parse: None,
        probes: &[],
        success: 200,
        success_description: "The revoked key.",
        response: schema_of::<RevokedKeyResponse>,
        errors: &[401, 403, 404, 410, 503],
        curl: "curl -X DELETE https://transgate.ai/api/keys/$KEY_ID -H 'Authorization: Bearer $SESSION_TOKEN'",
    },
    Operation {
        id: "getSigningSecret",
        method: "get",
        path: "/api/keys/signing-secret",
        summary: "Read the callback signing secret",
        description: "The secret every callback to your account is signed with (`Transgate-Signature: t=<unix>,v1=<hex HMAC-SHA256(secret, \"<t>.\" + body)>`). \
            Created on first use.",
        auth: Auth::Session,
        query: &[],
        headers: &[],
        path_params: &[],
        request: None,
        example: None,
        parse: None,
        probes: &[],
        success: 200,
        success_description: "The signing secret.",
        response: schema_of::<SigningSecretResponse>,
        errors: &[401, 403, 410, 503],
        curl: "curl https://transgate.ai/api/keys/signing-secret -H 'Authorization: Bearer $SESSION_TOKEN'",
    },
    Operation {
        id: "replayCallback",
        method: "post",
        path: "/api/admin/callbacks/{id}/replay",
        summary: "Replay a callback delivery",
        description: "Administrators only. A delivered, failed or abandoned delivery becomes due again under the same delivery id; \
            the next dispatch appends one attempt. A delivery that is still pending is `400`.",
        auth: Auth::Admin,
        query: &[],
        headers: &[],
        path_params: &[Param { name: "id", description: "The delivery id (the `Transgate-Delivery-Id` header receivers saw).", required: true }],
        request: None,
        example: None,
        parse: None,
        probes: &[],
        success: 200,
        success_description: "The delivery was reopened.",
        response: schema_of::<ReplayResponse>,
        errors: &[400, 401, 403, 404, 503],
        curl: "curl -X POST https://transgate.ai/api/admin/callbacks/$DELIVERY_ID/replay -H 'Authorization: Bearer $ADMIN_SESSION_TOKEN'",
    },
];

fn status_text(status: u16) -> &'static str {
    match status {
        400 => "The request is invalid.",
        401 => "Missing, malformed, unknown, inactive or expired credentials.",
        402 => "Not enough balance.",
        403 => "The credentials lack the required scope or permission.",
        404 => "Not found, or not yours.",
        409 => "`request_conflict`: this Idempotency-Key was already used for a different request.",
        410 => "The account is being deleted.",
        429 => "Rate limit exceeded: 60 requests per minute and 1000 per hour per key. See `Retry-After`.",
        503 => "A required store is unavailable. Retry later.",
        _ => "Error.",
    }
}

fn schema_value(schema: Schema) -> Value {
    serde_json::to_value(schema).unwrap_or(Value::Null)
}

fn parameter(location: &str, param: &Param) -> Value {
    json!({"name": param.name, "in": location, "required": param.required || location == "path", "description": param.description, "schema": {"type": "string"}})
}

fn rate_headers() -> Value {
    let header = |what: &str| json!({"description": what, "schema": {"type": "string"}});
    json!({
        "X-RateLimit-Limit-Minute": header("Requests allowed per minute."),
        "X-RateLimit-Limit-Hour": header("Requests allowed per hour."),
        "X-RateLimit-Remaining-Minute": header("Requests left in the current minute."),
        "X-RateLimit-Remaining-Hour": header("Requests left in the current hour."),
        "X-RateLimit-Reset-Minute": header("When the minute window resets (RFC 3339)."),
        "X-RateLimit-Reset-Hour": header("When the hour window resets (RFC 3339)."),
    })
}

fn operation_object(generator: &mut SchemaGenerator, op: &Operation) -> Value {
    let mut parameters: Vec<Value> = Vec::new();
    parameters.extend(op.path_params.iter().map(|p| parameter("path", p)));
    parameters.extend(op.query.iter().map(|p| parameter("query", p)));
    parameters.extend(op.headers.iter().map(|p| parameter("header", p)));
    let key_route = matches!(op.auth, Auth::Key(_));
    let mut responses = Map::new();
    let mut success = json!({
        "description": op.success_description,
        "content": {"application/json": {"schema": schema_value((op.response)(generator))}},
    });
    if key_route {
        success["headers"] = rate_headers();
    }
    responses.insert(op.success.to_string(), success);
    for status in op.errors {
        let mut error = json!({
            "description": status_text(*status),
            "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ErrorBody"}}},
        });
        if key_route && *status == 429 {
            error["headers"] = {
                let mut headers = rate_headers();
                headers["Retry-After"] = json!({"description": "Seconds until the exhausted window resets.", "schema": {"type": "string"}});
                headers
            };
        }
        responses.insert(status.to_string(), error);
    }
    let security = match op.auth {
        Auth::Key(_) => json!([{"apiKey": []}]),
        Auth::Session | Auth::Admin => json!([{"session": []}]),
    };
    let mut object = json!({
        "operationId": op.id,
        "summary": op.summary,
        "description": op.description,
        "security": security,
        "responses": Value::Object(responses),
        "x-codeSamples": [{"lang": "Shell", "label": "curl", "source": op.curl}],
    });
    if let Auth::Key(scope) = op.auth {
        object["x-required-scope"] = json!(scope);
    }
    if !parameters.is_empty() {
        object["parameters"] = Value::Array(parameters);
    }
    if let Some(request) = op.request {
        let mut content = json!({"schema": schema_value(request(generator))});
        if let Some(example) = op.example {
            content["example"] = serde_json::from_str(example).unwrap_or(Value::Null);
        }
        object["requestBody"] = json!({"required": true, "content": {"application/json": content}});
    }
    object
}

/// The OpenAPI 3.1 document of every operation in [`OPERATIONS`].
pub fn document() -> Value {
    let mut settings = SchemaSettings::draft2020_12();
    settings.definitions_path = "/components/schemas".into();
    settings.meta_schema = None;
    let mut generator = settings.into_generator();
    let mut paths: Map<String, Value> = Map::new();
    for op in OPERATIONS {
        let item = paths.entry(op.path.to_string()).or_insert_with(|| json!({}));
        item[op.method] = operation_object(&mut generator, op);
    }
    let _ = generator.subschema_for::<ErrorBody>();
    let schemas: Map<String, Value> = generator.take_definitions(true).into_iter().collect();
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Transgate API",
            "version": API_VERSION,
            "description": "Start transcriptions and read their results with an API key, and manage keys from the dashboard. \
                Every key request is limited to 60 per minute and 1000 per hour; the answer carries `X-RateLimit-*` headers. \
                Callbacks to a job's `callbackUrl` are POSTed as JSON with `Transgate-Delivery-Id` (stable across retries; use it to deduplicate) \
                and `Transgate-Signature: t=<unix seconds>,v1=<hex HMAC-SHA256(secret, \"<t>.\" + body)>`.",
        },
        "servers": [{"url": "/"}],
        "paths": Value::Object(paths),
        "components": {
            "schemas": Value::Object(schemas),
            "securitySchemes": {
                "apiKey": {"type": "http", "scheme": "bearer", "description": "An API key: `sk_` followed by 44 characters."},
                "session": {"type": "http", "scheme": "bearer", "bearerFormat": "JWT", "description": "The signed-in dashboard session."},
            },
        },
    })
}

#[cfg(test)]
mod tests;
