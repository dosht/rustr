mod support;
use std::time::Duration;
use base64::{Engine as _,engine::general_purpose::STANDARD};
use media_adapters::{Deepgram,DeepgramConfig,Soniox,SonioxConfig,HttpConfig};
use media_ports::*;
use serde_json::{Value,json};
use support::{Reply,Server};
use uuid::Uuid;

fn soniox(server:&Server)->Soniox {
    Soniox::new(SonioxConfig {base_url:server.url.clone(),api_key:SecretString::new("synthetic-key"),project:"project-a".into(),model:"stt-async-v5".into(),http:HttpConfig::default()}).unwrap()
}
fn deepgram(server:&Server)->Deepgram {
    Deepgram::new(DeepgramConfig {base_url:server.url.clone(),api_key:SecretString::new("synthetic-key"),project:"project-a".into(),http:HttpConfig::default()}).unwrap()
}
fn request(provider:Provider)->TranscriptionRequest {
    let attempt=Uuid::from_u128(42);
    TranscriptionRequest {attempt,provider,project:"project-a".into(),model:if provider==Provider::Soniox {"stt-async-v5"}else{"whisper-large"}.into(),
        language:Some("TR".into()),translation_target:None,diarize:true,client_reference:"job_73".into(),
        audio:TemporaryAccess {object:ObjectRef {store:StoreKind::Gcs,bucket:"private".into(),key:"userId=a/jobId=73/audio.wav".into(),version:None},
            method:AccessMethod::Get,url:SensitiveUrl::new("https://storage.example.test/private/audio?signature=private-audio"),expires_at:Micros(2_000_000),headers:vec![]},
        callback:CallbackSpec {url:SensitiveUrl::new(format!("https://host.example.test/callback?job_id=73&attempt={attempt}")),secret:SecretString::new("only-this-attempt")}}
}
fn remote(provider:Provider,id:&str)->RemoteIdentity {RemoteIdentity {provider,project:"project-a".into(),id:id.into()}}
fn binding(provider:Provider,id:&str)->CallbackBinding {CallbackBinding {attempt:Uuid::from_u128(42),provider,project:"project-a".into(),expected_remote_id:Some(id.into()),secret:SecretString::new("only-this-attempt")}}
fn soniox_headers()->Headers {vec![("X-Webhook-Secret".into(),"only-this-attempt".into())]}
fn deepgram_headers()->Headers {vec![("Authorization".into(),format!("Basic {}",STANDARD.encode(format!("{}:only-this-attempt",Uuid::from_u128(42)))))]}
fn transcript()->Value {json!({"id":"remote-1","text":"Merhaba Hello", "tokens":[
    {"text":"Merhaba","start_ms":0,"end_ms":1200,"speaker":"1","language":"tr","translation_status":"original","confidence":0.97,"is_audio_event":null},
    {"text":"Hello","start_ms":0,"end_ms":0,"speaker":1,"language":"en","translation_status":"translation","source_language":"tr"},
    {"text":"[music]","start_ms":null,"end_ms":null,"is_audio_event":true}],"metadata":{"source":"fixture"}})}
fn deepgram_payload()->Value {json!({"metadata":{"request_id":"remote-1","duration":61.25,"sha256":"source-hash","extra":{"retained":"yes"}},
    "results":{"channels":[{"detected_language":"tr","alternatives":[{"transcript":"Hello world.","confidence":0.9,
        "words":[{"word":"hello","punctuated_word":"Hello","start":0.25,"end":1.0,"speaker":0,"confidence":0.98,"speaker_confidence":0.77}],
        "paragraphs":{"transcript":"Hello world.","paragraphs":[{"speaker":0,"start":0.25,"end":1.25,"num_words":2,
            "sentences":[{"text":"Hello world.","start":0.25,"end":1.25}]}]}}]}]},"extra_top":{"keep":true}})}

#[tokio::test]
async fn soniox_real_post_keeps_source_reference_and_explicit_v5_language_translation_auth() {
    let server=Server::start().await;
    let adapter=soniox(&server);
    server.reply(Reply::json(201,json!({"id":"remote-1"})));
    let mut input=request(Provider::Soniox);
    input.translation_target=Some("EN".into());
    assert_eq!(adapter.submit(&input).await.unwrap(),Submission::Accepted {remote:remote(Provider::Soniox,"remote-1")});
    let sent=server.requests().pop().unwrap();
    assert_eq!((sent.method.as_str(),sent.path.as_str()),("POST","/v1/transcriptions"));
    assert_eq!(sent.headers["authorization"],"Bearer synthetic-key");
    let body:Value=serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(body["model"],"stt-async-v5");
    assert_eq!(body["client_reference_id"],"job_73");
    assert_eq!(body["language_hints"],json!(["tr"]));
    assert_eq!(body["translation"],json!({"type":"one_way","target_language":"en"}));
    assert_eq!(body["enable_language_identification"],true);
    assert_eq!(body["enable_speaker_diarization"],true);
    assert_eq!(body["webhook_auth_header_name"],"X-Webhook-Secret");
    assert_eq!(body["webhook_auth_header_value"],"only-this-attempt");
    assert_eq!(body["webhook_url"],input.callback.url.expose());
    assert_eq!(body["audio_url"],input.audio.url.expose());
    input.language=None;
    server.reply(Reply::json(201,json!({"id":"remote-auto"})));
    adapter.submit(&input).await.unwrap();
    let auto:Value=serde_json::from_slice(&server.requests()[1].body).unwrap();
    assert!(auto.get("language_hints").is_none(),"auto detect must not receive a forcing single-language hint");
}

#[tokio::test]
async fn d6_soniox_callback_preserves_received_duration_and_full_tokens_without_status_enrichment() {
    let server=Server::start().await;
    let adapter=soniox(&server);
    for received in [None,Some(61001.0)] {
        server.reply(Reply::json(200,transcript()));
        let mut raw=json!({"id":"remote-1","status":"completed","wire_extra":{"retain":true}});
        if let Some(duration)=received {raw["audio_duration_ms"]=json!(duration);}
        let observation=adapter.verify_callback(&binding(Provider::Soniox,"remote-1"),&soniox_headers(),&serde_json::to_vec(&raw).unwrap()).unwrap();
        let completed=adapter.complete_callback(observation).await.unwrap();
        assert_eq!(completed.origin,ObservationOrigin::Webhook);
        let ProviderPayload::Soniox {reported,transcript:Some(tokens)}=completed.payload else {panic!("wrong payload")};
        assert_eq!(reported.audio_duration_ms,received);
        assert_eq!(reported.extra["wire_extra"],json!({"retain":true}));
        assert_eq!(tokens.tokens[0].speaker,Some(json!("1")));
        assert_eq!(tokens.tokens[1].speaker,Some(json!(1)));
        assert_eq!(tokens.tokens[1].translation_status.as_deref(),Some("translation"));
        assert_eq!(tokens.tokens[1].extra["source_language"],"tr");
        assert_eq!(tokens.tokens[2].start_ms,None);
        assert_eq!(tokens.tokens[2].is_audio_event,Some(true));
        let restored:SonioxTranscript=serde_json::from_slice(&serde_json::to_vec(&tokens).unwrap()).unwrap();
        assert_eq!(restored,tokens,"durable intake must not discard provider distinctions");
    }
    assert_eq!(server.requests().iter().map(|r|r.path.as_str()).collect::<Vec<_>>(),vec!["/v1/transcriptions/remote-1/transcript","/v1/transcriptions/remote-1/transcript"]);
}

#[tokio::test]
async fn recovery_poll_owns_its_duration_and_mismatched_transcript_identity_is_rejected() {
    let server=Server::start().await;
    let adapter=soniox(&server);
    server.reply(Reply::json(200,json!({"id":"remote-1","status":"completed","audio_duration_ms":95000})));
    server.reply(Reply::json(200,transcript()));
    let result=adapter.result(&remote(Provider::Soniox,"remote-1")).await.unwrap();
    assert_eq!(result.origin,ObservationOrigin::RecoveryPoll);
    assert!(matches!(result.payload,ProviderPayload::Soniox{reported:SonioxReported{audio_duration_ms:Some(95000.0),..},transcript:Some(_)}));
    let mut wrong=transcript();wrong["id"]=json!("another-customer");
    server.reply(Reply::json(200,wrong));
    let observation=adapter.verify_callback(&binding(Provider::Soniox,"remote-1"),&soniox_headers(),br#"{"id":"remote-1","status":"completed"}"#).unwrap();
    assert!(matches!(adapter.complete_callback(observation).await,Err(TransportError::Malformed(_))));
}

#[tokio::test]
async fn callbacks_authenticate_before_parse_and_bind_provider_project_remote_with_full_deepgram_body() {
    let server=Server::start().await;
    let sx=soniox(&server);let dg=deepgram(&server);
    assert_eq!(sx.verify_callback(&binding(Provider::Soniox,"remote-1"),&vec![],b"not json"),Err(CallbackError::Unauthorized));
    assert_eq!(dg.verify_callback(&binding(Provider::Deepgram,"remote-1"),&vec![],b"not json"),Err(CallbackError::Unauthorized));
    let raw=serde_json::to_vec(&deepgram_payload()).unwrap();
    let good=binding(Provider::Deepgram,"remote-1");
    let observation=dg.verify_callback(&good,&deepgram_headers(),&raw).unwrap();
    let ProviderPayload::Deepgram(payload)=&observation.payload else {panic!("wrong payload")};
    assert_eq!(serde_json::to_value(payload).unwrap(),deepgram_payload(),"entire callback must survive, not only first-channel sentences");
    assert_eq!(dg.complete_callback(observation.clone()).await.unwrap(),observation);
    for wrong in [CallbackBinding{expected_remote_id:Some("wrong-id".into()),..good.clone()},
        CallbackBinding{project:"wrong-project".into(),..good.clone()},
        CallbackBinding{provider:Provider::Soniox,..good.clone()}] {
        assert_eq!(dg.verify_callback(&wrong,&deepgram_headers(),&raw),Err(CallbackError::IdentityMismatch));
    }
    let mut duplicate=deepgram_headers();duplicate.extend(deepgram_headers());
    assert_eq!(dg.verify_callback(&good,&duplicate,&raw),Err(CallbackError::Unauthorized));
    let early=CallbackBinding{expected_remote_id:None,..good};
    assert_eq!(dg.verify_callback(&early,&deepgram_headers(),&raw).unwrap().remote,remote(Provider::Deepgram,"remote-1"));
    assert_eq!(dg.verify_callback(&early,&vec![],&raw),Err(CallbackError::Unauthorized));
    let early_soniox=CallbackBinding{expected_remote_id:None,..binding(Provider::Soniox,"unused")};
    assert_eq!(sx.verify_callback(&early_soniox,&soniox_headers(),br#"{"id":"first-observed","status":"completed"}"#).unwrap().remote,
        remote(Provider::Soniox,"first-observed"));
    assert!(server.requests().is_empty(),"callback verification and Deepgram completion never fetch/reconstruct a result");
}

#[tokio::test]
async fn d3_d4_deepgram_global_whisper_request_is_opted_out_and_basic_authenticated() {
    let server=Server::start().await;
    let adapter=deepgram(&server);
    // Captured 61-code offered-language table; transport acceptance is not live model support.
    for language in [
        "af","ar","hy","az","be","bs","bg","ca","zh","hr","cs","da","nl","en","et","fi","fr","gl","de","el",
        "he","hi","hu","is","id","it","ja","kn","kk","ko","lv","lt","mk","ms","mr","mi","ne","no","fa","pl",
        "pt","ro","ru","sr","sk","sl","es","sw","sv","tl","ta","th","tr","uk","ur","vi","cy","ka","la","uz","lb",
    ] {
        server.reply(Reply::json(200,json!({"request_id":"remote-1"})));
        let mut input=request(Provider::Deepgram);input.language=Some(language.into());
        assert!(matches!(adapter.submit(&input).await.unwrap(),Submission::Accepted{..}));
        let sent=server.requests().pop().unwrap();
        assert_eq!((sent.method.as_str(),sent.path.as_str()),("POST","/v1/listen"));
        assert_eq!(sent.headers["authorization"],"Token synthetic-key");
        let query:std::collections::HashMap<_,_>=url::form_urlencoded::parse(sent.query.as_bytes()).into_owned().collect();
        assert_eq!(query["language"],language);
        assert_eq!(query["model"],"whisper-large");
        for flag in ["mip_opt_out","smart_format","punctuate","paragraphs","diarize"] {assert_eq!(query[flag],"true");}
        let callback=url::Url::parse(&query["callback"]).unwrap();
        assert_eq!(callback.username(),input.attempt.to_string());
        assert_eq!(callback.password(),Some("only-this-attempt"));
        assert_eq!(serde_json::from_slice::<Value>(&sent.body).unwrap(),json!({"url":input.audio.url.expose()}));
    }
    assert!(matches!(adapter.status(&remote(Provider::Deepgram,"remote-1")).await,Err(TransportError::Unsupported(_))));
    assert!(matches!(adapter.delete_remote(&remote(Provider::Deepgram,"remote-1")).await.unwrap(),DeleteOutcome::Unsupported{..}));
}

#[tokio::test]
async fn post_ambiguity_does_not_become_rejection_or_retry_and_explicit_429_is_rejected() {
    let server=Server::start().await;
    let adapter=Soniox::new(SonioxConfig{base_url:server.url.clone(),api_key:SecretString::new("k"),project:"project-a".into(),model:"stt-async-v5".into(),
        http:HttpConfig{request_timeout:Duration::from_millis(40),..HttpConfig::default()}}).unwrap();
    let cases=[Reply::json(200,json!({"not":"an id"})),Reply::text(200,"bad json"),Reply::json(503,json!({"id":"maybe"})),
        Reply::json(200,json!({"id":"too-late"})).delayed(Duration::from_millis(200))];
    for reply in cases {server.reply(reply);assert!(matches!(adapter.submit(&request(Provider::Soniox)).await.unwrap(),Submission::Uncertain{..}));}
    server.reply(Reply::json(429,json!({"error_type":"limit_exceeded"})));
    assert!(matches!(adapter.submit(&request(Provider::Soniox)).await.unwrap(),Submission::Rejected{status:Some(429),..}));
    assert_eq!(server.requests().len(),5,"one POST per invocation, never hidden retry/failover");
}

#[tokio::test]
async fn soniox_listing_preserves_all_ambiguous_matches_and_delete_processing_is_not_success() {
    let server=Server::start().await;let adapter=soniox(&server);
    let row=|id:&str,reference:&str|json!({"id":id,"status":"processing","model":"stt-async-v5","created_at":"2026-10-24T00:00:00Z",
        "client_reference_id":reference,"audio_url":"https://store.test/a?secret=one","webhook_url":format!("https://host.test/callback?attempt={id}")});
    server.reply(Reply::json(200,json!({"transcriptions":[row("remote-1","job_73"),row("other","job_99")],"next_page_cursor":"next-page"})));
    server.reply(Reply::json(200,json!({"transcriptions":[row("remote-2","job_73")],"next_page_cursor":null})));
    let matches=adapter.find_by_reference("job_73").await.unwrap();
    assert_eq!(matches.iter().map(|r|r.remote.id.as_str()).collect::<Vec<_>>(),vec!["remote-1","remote-2"]);
    assert!(server.requests()[1].query.contains("cursor=next-page"));
    assert!(matches[1].callback_url.as_ref().unwrap().expose().ends_with("attempt=remote-2"));
    server.reply(Reply::json(409,json!({"error_type":"transcription_invalid_state"})));
    assert!(matches!(adapter.delete_remote(&remote(Provider::Soniox,"remote-1")).await.unwrap(),DeleteOutcome::Retained{..}));
    server.reply(Reply::text(204,""));
    assert_eq!(adapter.delete_remote(&remote(Provider::Soniox,"remote-1")).await.unwrap(),DeleteOutcome::Deleted);
}

#[test]
fn secrets_and_provider_metadata_are_redacted_from_debug_but_durable_payload_is_not_truncated() {
    let request=request(Provider::Deepgram);
    let printed=format!("{request:?}");
    assert!(!printed.contains("only-this-attempt"));assert!(!printed.contains("private-audio"));
    let reported:SonioxReported=serde_json::from_value(json!({"id":"r","status":"completed","audio_url":"https://store.test/?secret=hidden"})).unwrap();
    assert!(!format!("{reported:?}").contains("hidden"));
    assert_eq!(serde_json::to_value(reported).unwrap()["audio_url"],"https://store.test/?secret=hidden");
}
