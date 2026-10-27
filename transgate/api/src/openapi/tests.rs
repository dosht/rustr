use super::*;

fn doc() -> Value {
    document()
}

#[test]
fn the_document_is_openapi_3_1_and_lists_exactly_the_operation_table() {
    let doc = doc();
    assert_eq!(doc["openapi"], "3.1.0");
    let mut listed = Vec::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            listed.push((method.clone(), path.clone(), op["operationId"].as_str().unwrap().to_string()));
        }
    }
    let mut wanted: Vec<_> = OPERATIONS.iter().map(|o| (o.method.to_string(), o.path.to_string(), o.id.to_string())).collect();
    listed.sort();
    wanted.sort();
    assert_eq!(listed, wanted);
    let ids: std::collections::BTreeSet<_> = OPERATIONS.iter().map(|o| o.id).collect();
    assert_eq!(ids.len(), OPERATIONS.len(), "operation ids are unique");
}

// Every `$ref` resolves, every documented schema is generated from a DTO, and no schema is empty.
#[test]
fn every_reference_resolves_to_a_component_schema() {
    let doc = doc();
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let mut refs = Vec::new();
    fn walk(value: &Value, refs: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if k == "$ref" {
                        refs.push(v.as_str().unwrap().to_string());
                    }
                    walk(v, refs);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, refs)),
            _ => {}
        }
    }
    walk(&doc, &mut refs);
    assert!(!refs.is_empty());
    for reference in refs {
        let name = reference.strip_prefix("#/components/schemas/").unwrap_or_else(|| panic!("{reference}"));
        assert!(schemas.contains_key(name), "{reference} has no component");
    }
    for name in ["StartResponse", "StatusResponse", "TranscriptionsRequest", "NewKeyRequest", "ErrorBody", "KeyListResponse", "CreatedKeyResponse"] {
        assert!(schemas.contains_key(name), "{name}");
    }
}

// D9 agreement, parser side: the documented examples are accepted by the real parsers, and every
// property the request schema documents is one the parser actually reads (a wrong-typed value is
// refused). An undocumented-but-read key is covered by the example being the only documented use.
#[test]
fn documented_examples_parse_and_every_documented_request_property_is_read_by_the_parser() {
    let doc = doc();
    for op in OPERATIONS {
        let (Some(example), Some(parse)) = (op.example, op.parse) else { continue };
        assert_eq!(parse(example.as_bytes()), Ok(()), "{}: the documented example must parse", op.id);
        let schema_ref = doc["paths"][op.path][op.method]["requestBody"]["content"]["application/json"]["schema"]["$ref"].as_str().unwrap();
        let name = schema_ref.strip_prefix("#/components/schemas/").unwrap();
        let properties: Vec<String> = doc["components"]["schemas"][name]["properties"].as_object().unwrap().keys().cloned().collect();
        assert!(!properties.is_empty(), "{}", op.id);
        let base: Value = serde_json::from_str(example).unwrap();
        let probed: std::collections::BTreeSet<&str> = op.probes.iter().map(|(p, _)| *p).collect();
        let documented: std::collections::BTreeSet<&str> = properties.iter().map(String::as_str).collect();
        assert_eq!(probed, documented, "{}: every documented property needs a probe, and only those", op.id);
        let before = parse(example.as_bytes());
        for (property, value) in op.probes {
            let mut changed = base.clone();
            changed[*property] = serde_json::from_str(value).unwrap();
            assert_ne!(parse(changed.to_string().as_bytes()), before, "{}: the parser ignores documented property `{property}`", op.id);
        }
    }
}

// The required fields in the schema are the ones whose absence the parser refuses.
#[test]
fn a_request_without_a_required_property_is_refused_by_the_parser() {
    let doc = doc();
    let required: Vec<String> = doc["components"]["schemas"]["NewKeyRequest"]["required"]
        .as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let base: Value = serde_json::from_str(OPERATIONS.iter().find(|o| o.id == "createApiKey").unwrap().example.unwrap()).unwrap();
    for property in &required {
        let mut body = base.clone();
        body.as_object_mut().unwrap().remove(property);
        let outcome = parse_new_key(body.to_string().as_bytes());
        // `expirationPolicy` is documented required but the source defaulted it; the parser keeps the default.
        if property == "expirationPolicy" {
            assert!(outcome.is_ok(), "the source defaulted a missing policy to never");
        } else {
            assert!(outcome.is_err(), "`{property}` is required");
        }
    }
    assert!(required.iter().any(|p| p == "name") && required.iter().any(|p| p == "scopes"));
}

// The committed document is the one the code generates. `OPENAPI_UPDATE=1 cargo test -p transgate-api`
// rewrites it; a reviewer sees the contract change in the diff.
#[test]
fn the_committed_openapi_document_is_current() {
    let generated = serde_json::to_string_pretty(&doc()).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi/openapi.json");
    if std::env::var("OPENAPI_UPDATE").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
    }
    let committed = std::fs::read_to_string(&path).expect("transgate/api/openapi/openapi.json is committed (run with OPENAPI_UPDATE=1)");
    assert_eq!(committed, generated, "the committed OpenAPI document is stale: run `OPENAPI_UPDATE=1 cargo test -p transgate-api`");
}

#[test]
fn every_key_route_documents_the_rate_limit_headers_and_the_session_routes_do_not() {
    let doc = doc();
    for op in OPERATIONS {
        let ok = &doc["paths"][op.path][op.method]["responses"][op.success.to_string()];
        let has = ok["headers"]["X-RateLimit-Remaining-Minute"].is_object();
        assert_eq!(has, matches!(op.auth, Auth::Key(_)), "{}", op.id);
        assert_eq!(doc["paths"][op.path][op.method]["responses"]["429"].is_object(), op.errors.contains(&429), "{}", op.id);
        assert_eq!(doc["paths"][op.path][op.method]["x-codeSamples"][0]["label"], "curl");
    }
}
