mod support;
use media_adapters::*;
use media_ports::*;
use serde_json::json;
use support::{Reply,Server};
use hmac::{Hmac,Mac};
use sha2::{Digest,Sha256};

fn object(kind:StoreKind)->ObjectRef {ObjectRef{store:kind,bucket:"private".into(),key:"userId=a/jobId=73/audio clip.wav".into(),version:None}}
fn store(server:&Server,kind:StoreKind)->S3Store {
    S3Store::new(S3StoreConfig{kind,endpoint:server.url.clone(),bucket:"private".into(),region:"auto".into(),access_key:SecretString::new("fixture-access"),secret_key:SecretString::new("fixture-secret"),http:HttpConfig::default()}).unwrap()
}
fn mac(key:&[u8],value:&str)->Vec<u8> {let mut h=Hmac::<Sha256>::new_from_slice(key).unwrap();h.update(value.as_bytes());h.finalize().into_bytes().to_vec()}
// Independent AWS SigV4 verifier, not rusty-s3 re-signing its own output.
fn signature_matches(url:&url::Url,method:&str)->bool {
    let pairs:std::collections::HashMap<_,_>=url.query_pairs().into_owned().collect();
    let mut query:Vec<_>=url.query().unwrap().split('&').filter(|part|!part.starts_with("X-Amz-Signature=")).collect();query.sort_unstable();
    let host=match url.port(){Some(port)=>format!("{}:{port}",url.host_str().unwrap()),None=>url.host_str().unwrap().into()};
    let canonical=format!("{method}\n{}\n{}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD",url.path(),query.join("&"));
    let (_,scope)=pairs["X-Amz-Credential"].split_once('/').unwrap();
    let string_to_sign=format!("AWS4-HMAC-SHA256\n{}\n{scope}\n{:x}",pairs["X-Amz-Date"],Sha256::digest(canonical.as_bytes()));
    let mut key=b"AWS4fixture-secret".to_vec();for part in scope.split('/') {key=mac(&key,part);}
    let actual=mac(&key,&string_to_sign).iter().map(|b|format!("{b:02x}")).collect::<String>();
    actual==pairs["X-Amz-Signature"]
}

#[tokio::test]
async fn gcs_and_r2_signed_access_binds_key_method_expiry_without_content_type_or_resumable_upload() {
    let server=Server::start().await;
    for kind in [StoreKind::Gcs,StoreKind::R2] {
        let store=store(&server,kind);let object=object(kind);
        for (method,verb,ttl) in [(AccessMethod::Put,"PUT",900),(AccessMethod::Get,"GET",900),(AccessMethod::Get,"GET",3600)] {
            let access=store.sign(&object,method,Micros(1_700_000_000_000_000),ttl).await.unwrap();
            let url=url::Url::parse(access.url.expose()).unwrap();
            assert!(signature_matches(&url,verb),"independent verifier must authenticate the signed capability");
            assert!(!signature_matches(&url,if verb=="PUT"{"GET"}else{"PUT"}));
            let mut tampered=url.clone();tampered.set_path("/private/another-customer.wav");
            assert!(!signature_matches(&tampered,verb));
            let query:std::collections::HashMap<_,_>=url.query_pairs().into_owned().collect();
            assert_eq!(query["X-Amz-Expires"],ttl.to_string());
            assert_eq!(query["X-Amz-SignedHeaders"],"host");
            assert!(access.headers.is_empty());
            assert_eq!(access.expires_at,Micros(1_700_000_000_000_000+i64::from(ttl)*1_000_000));
            assert_eq!(url.path(),"/private/userId%3Da/jobId%3D73/audio%20clip.wav");
        }
        let mut wrong=object.clone();wrong.bucket="other-account".into();
        assert!(matches!(store.sign(&wrong,AccessMethod::Get,Micros(1),900).await,Err(TransportError::Rejected{..})));
    }
    assert!(server.requests().is_empty(),"HMAC signing is local, not a provider/IAM call");
}

#[tokio::test]
async fn real_storage_put_head_and_versioned_delete_keep_observed_object_identity() {
    let server=Server::start().await;let store=store(&server,StoreKind::Gcs);let object=object(StoreKind::Gcs);
    server.reply(Reply::text(200,"").header("x-goog-generation","123").header("etag","etag-a"));
    let body=futures_util::stream::iter([Ok(bytes::Bytes::from_static(b"abc")),Ok(bytes::Bytes::from_static(b"def"))]);
    let uploaded=store.put(PutObject{object:object.clone(),body:Box::pin(body),content_length:6,content_type:"audio/wav".into()}).await.unwrap();
    assert_eq!(uploaded.object.version.as_deref(),Some("123"));assert_eq!(uploaded.size,6);
    let sent=&server.requests()[0];assert_eq!(sent.method,"PUT");assert_eq!(sent.body,b"abcdef");assert_eq!(sent.headers["content-length"],"6");
    server.reply(Reply::text(200,"").header("content-length","6").header("content-type","audio/wav").header("x-goog-generation","123"));
    assert_eq!(store.head(&uploaded.object).await.unwrap().unwrap().object,uploaded.object);
    server.reply(Reply::text(204,""));
    assert_eq!(store.delete(&uploaded.object).await.unwrap(),DeleteOutcome::Deleted);
    let requests=server.requests();assert_eq!(requests[2].method,"DELETE");
    assert!(requests[2].query.split('&').any(|pair|pair=="generation=123"));
    let url=url::Url::parse(&format!("{}{}?{}",server.url,requests[2].path,requests[2].query)).unwrap();
    assert!(signature_matches(&url,"DELETE"));
}

#[tokio::test]
async fn retention_absence_permission_and_provider_failures_are_not_all_deleted() {
    let server=Server::start().await;let store=store(&server,StoreKind::Gcs);let object=object(StoreKind::Gcs);
    server.reply(Reply::text(403,"<Error><Code>AccessDenied</Code><Message>Object is under active retention policy</Message></Error>"));
    assert!(matches!(store.delete(&object).await.unwrap(),DeleteOutcome::Retained{..}));
    server.reply(Reply::text(403,"<Error><Code>AccessDenied</Code><Message>permission denied</Message></Error>"));
    assert!(matches!(store.delete(&object).await,Err(TransportError::Unavailable{status:Some(403),..})));
    server.reply(Reply::text(404,"missing"));assert_eq!(store.delete(&object).await.unwrap(),DeleteOutcome::Absent);
    server.reply(Reply::text(503,"unavailable"));assert!(store.delete(&object).await.is_err());
    assert_eq!(server.requests().len(),4,"no internal delete retries");
}

#[tokio::test]
async fn d2_legacy_supabase_inventory_and_delete_use_service_authority_and_confirm_empty_success() {
    let server=Server::start().await;
    let store=LegacySupabaseStorage::new(LegacySupabaseConfig{base_url:server.url.clone(),service_key:SecretString::new("legacy-service"),http:HttpConfig::default()}).unwrap();
    server.reply(Reply::json(200,json!([{"name":"job-73","id":null,"metadata":null},{"name":"audio.wav","id":"object-id","metadata":{"size":5}}])));
    let entries=store.list_prefix("account-a",0,100).await.unwrap();
    assert_eq!(entries[0].id,None);assert_eq!(entries[1].id.as_deref(),Some("object-id"));
    let object=ObjectRef{store:StoreKind::Supabase,bucket:"jobs".into(),key:"account-a/job-73/audio.wav".into(),version:None};
    server.reply(Reply::json(200,json!([{"name":object.key}])));
    assert_eq!(store.delete(&object).await.unwrap(),DeleteOutcome::Deleted);
    let sent=&server.requests()[1];assert_eq!((sent.method.as_str(),sent.path.as_str()),("DELETE","/storage/v1/object/jobs"));
    assert_eq!(sent.headers["authorization"],"Bearer legacy-service");assert_eq!(sent.headers["apikey"],"legacy-service");
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&sent.body).unwrap(),json!({"prefixes":[object.key]}));
    server.reply(Reply::json(200,json!([])));server.reply(Reply::json(404,json!({"error":"not found"})));
    assert_eq!(store.delete(&object).await.unwrap(),DeleteOutcome::Absent);
    server.reply(Reply::json(200,json!([])));
    server.reply(Reply::json(200,json!({"size":5,"content_type":"audio/wav","etag":"object-etag","metadata":{}})));
    assert_eq!(store.delete(&object).await,Err(TransportError::Unavailable {
        status:None,operation:"legacy delete not confirmed".into(),
    }),"a valid lookup of an existing object must report unconfirmed deletion, not a parse error");
    server.reply(Reply::json(200,json!([])));server.reply(Reply::json(503,json!({"error":"unavailable"})));
    assert_eq!(store.delete(&object).await,Err(TransportError::Unavailable {
        status:Some(503),operation:"legacy info".into(),
    }),"failed lookup is neither authenticated absence nor confirmed presence");
    server.reply(Reply::json(200,json!([])));
    server.reply(Reply::json(200,json!({"size":null,"metadata":{"size":5}})));
    assert_eq!(store.delete(&object).await,Err(TransportError::Malformed("legacy object size".into())));
    let count=server.requests().len();let mut wrong=object.clone();wrong.bucket="avatars".into();
    assert!(store.delete(&wrong).await.is_err());assert_eq!(server.requests().len(),count);
    assert!(matches!(store.sign(&object,AccessMethod::Put,Micros(0),900).await,Err(TransportError::Unsupported(_))));
}

#[tokio::test]
async fn d2_legacy_supabase_info_reads_root_storage_fields_not_user_metadata() {
    let server=Server::start().await;
    let store=LegacySupabaseStorage::new(LegacySupabaseConfig{base_url:server.url.clone(),service_key:SecretString::new("legacy-service"),http:HttpConfig::default()}).unwrap();
    let object=ObjectRef{store:StoreKind::Supabase,bucket:"jobs".into(),key:"account-a/job-73/audio.wav".into(),version:None};
    // Official InfoRenderer: storage properties are root fields; metadata is user-supplied.
    for metadata in [json!({}),json!(null),json!({"size":999,"mimetype":"text/plain","content_type":"text/plain","etag":"forged","eTag":"forged"})] {
        server.reply(Reply::json(200,json!({
            "id":"object-id","name":object.key,"bucket_id":"jobs","version":"object-version",
            "size":5,"content_type":"audio/wav","etag":"object-etag","metadata":metadata,
        })));
        let info=store.head(&object).await.unwrap().unwrap();
        assert_eq!(info.object,object);
        assert_eq!((info.size,info.content_type.as_deref(),info.etag.as_deref()),(5,Some("audio/wav"),Some("object-etag")));
    }
    server.reply(Reply::json(200,json!({"size":0,"content_type":null,"etag":null,"metadata":{"size":999}})));
    let empty=store.head(&object).await.unwrap().unwrap();
    assert_eq!((empty.size,empty.content_type,empty.etag),(0,None,None));
}

fn legacy(server:&Server)->std::sync::Arc<dyn ObjectInventory> {
    std::sync::Arc::new(LegacySupabaseStorage::new(LegacySupabaseConfig{base_url:server.url.clone(),service_key:SecretString::new("legacy-service"),http:HttpConfig::default()}).unwrap())
}
fn file(name:&str)->serde_json::Value {json!({"name":name,"id":format!("id-{name}"),"metadata":{"size":1}})}
fn folder(name:&str)->serde_json::Value {json!({"name":name,"id":null,"metadata":null})}
fn list_body(request:&support::Recorded)->serde_json::Value {serde_json::from_slice(&request.body).unwrap()}

#[tokio::test]
async fn d2_legacy_inventory_pages_every_folder_of_one_owner_and_feeds_exact_deletions() {
    let server=Server::start().await;let inventory=legacy(&server);
    // A full first page (999 archives and one job folder), then a short second page.
    let mut first:Vec<_>=(0..999).map(|i|file(&format!("loose-{i:04}.json"))).collect();first.push(folder("job-73"));
    server.reply(Reply::json(200,serde_json::Value::Array(first)));
    server.reply(Reply::json(200,json!([file("zz-last.json")])));
    server.reply(Reply::json(200,json!([file("soniox_transcript.json"),file("audio.wav")])));
    let objects=inventory.inventory("account-a").await.unwrap();
    assert_eq!((inventory.kind(),inventory.bucket()),(StoreKind::Supabase,"jobs"));
    assert_eq!(objects.len(),1002,"both pages of the owner folder and the nested job folder");
    assert!(objects.iter().all(|o|o.store==StoreKind::Supabase&&o.bucket=="jobs"&&o.version.is_none()&&o.key.starts_with("account-a/")));
    assert!(objects.windows(2).all(|w|w[0].key<w[1].key),"deterministic key order");
    for key in ["account-a/job-73/audio.wav","account-a/job-73/soniox_transcript.json","account-a/zz-last.json","account-a/loose-0998.json"] {
        assert!(objects.iter().any(|o|o.key==key),"{key}");
    }
    assert!(!objects.iter().any(|o|o.key=="account-a/job-73"),"a folder is not an object");
    let requests=server.requests();assert_eq!(requests.len(),3);
    for (request,(prefix,offset)) in requests.iter().zip([("account-a",0),("account-a",1000),("account-a/job-73",0)]) {
        assert_eq!((request.method.as_str(),request.path.as_str()),("POST","/storage/v1/object/list/jobs"));
        assert_eq!(request.headers["authorization"],"Bearer legacy-service");assert_eq!(request.headers["apikey"],"legacy-service");
        assert_eq!(list_body(request),json!({"prefix":prefix,"offset":offset,"limit":1000,"sortBy":{"column":"name","order":"asc"}}));
    }
    // What the inventory returns is exactly what the same authority deletes and confirms.
    let store=LegacySupabaseStorage::new(LegacySupabaseConfig{base_url:server.url.clone(),service_key:SecretString::new("legacy-service"),http:HttpConfig::default()}).unwrap();
    let target=objects.iter().find(|o|o.key=="account-a/job-73/soniox_transcript.json").unwrap();
    server.reply(Reply::json(200,json!([{"name":target.key}])));
    assert_eq!(store.delete(target).await.unwrap(),DeleteOutcome::Deleted);
    assert_eq!(list_body(&server.requests()[3]),json!({"prefixes":["account-a/job-73/soniox_transcript.json"]}));
    server.reply(Reply::json(200,json!([])));
    assert_eq!(inventory.inventory("account-a").await.unwrap(),vec![],"an authenticated empty listing is the only empty answer");
}

#[tokio::test]
async fn d2_legacy_inventory_rejects_detected_listing_failures_instead_of_partial_results() {
    let server=Server::start().await;let inventory=legacy(&server);
    // Owner scope is one segment; nothing is requested for anything else.
    for owner in ["","account-a/job-73","..","account-a/..","/account-a"] {
        assert_eq!(inventory.inventory(owner).await,Err(TransportError::Rejected{status:None,code:"legacy_inventory_scope".into()}),"{owner:?}");
    }
    assert!(server.requests().is_empty());
    // A later page or a nested folder that cannot be listed loses the whole inventory.
    let full:Vec<_>=(0..1000).map(|i|file(&format!("f-{i:04}"))).collect();
    server.reply(Reply::json(200,serde_json::Value::Array(full.clone())));
    server.reply(Reply::json(503,json!({"error":"unavailable"})));
    assert_eq!(inventory.inventory("account-a").await,Err(TransportError::Unavailable{status:Some(503),operation:"legacy list".into()}));
    server.reply(Reply::json(200,json!([folder("job-1"),file("a.json")])));
    server.reply(Reply::json(401,json!({"error":"unauthorized"})));
    assert_eq!(inventory.inventory("account-a").await,Err(TransportError::Unavailable{status:Some(401),operation:"legacy list".into()}));
    // A folder that shifted between pages repeats a name: not a complete listing.
    let mut shifted:Vec<_>=(0..999).map(|i|file(&format!("g-{i:04}"))).collect();shifted.push(file("g-0999"));
    server.reply(Reply::json(200,serde_json::Value::Array(shifted)));
    server.reply(Reply::json(200,json!([file("g-0999"),file("g-1000")])));
    assert_eq!(inventory.inventory("account-a").await,Err(TransportError::Unavailable{status:None,operation:"legacy list changed during inventory".into()}));
    // Names that would leave the owner prefix, and pages larger than requested, are malformed.
    for name in ["../account-b","job/escape",".."] {
        server.reply(Reply::json(200,json!([file(name)])));
        assert_eq!(inventory.inventory("account-a").await,Err(TransportError::Malformed("legacy inventory path".into())),"{name}");
    }
    server.reply(Reply::json(200,serde_json::Value::Array((0..1001).map(|i|file(&format!("h-{i:04}"))).collect())));
    assert_eq!(inventory.inventory("account-a").await,Err(TransportError::Malformed("legacy list page size".into())));
    // A tree deeper than the legacy layout allows is refused rather than cut off.
    for level in 1..media_adapters::storage::LEGACY_INVENTORY_DEPTH {server.reply(Reply::json(200,json!([folder(&format!("d{level}"))])));}
    server.reply(Reply::json(200,json!([folder("too-deep")])));
    assert_eq!(inventory.inventory("account-a").await,Err(TransportError::LimitExceeded("legacy inventory depth".into())));
    assert_eq!(server.pending(),0,"every scripted page was requested");
    server.reply(Reply::text(200,"not json"));
    assert_eq!(inventory.inventory("account-a").await,Err(TransportError::Malformed("legacy list".into())));
}
