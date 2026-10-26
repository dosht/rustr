mod support;
use media_adapters::*;
use media_ports::*;
use serde_json::{Value,json};
use support::{Reply,Server};
const VIDEO:&str="aB_cD-eF123";
fn paid(server:&Server)->TranscriptApi {TranscriptApi::new(TranscriptApiConfig{base_url:server.url.clone(),api_key:SecretString::new("paid-key"),http:HttpConfig::default()}).unwrap()}
fn public(server:&Server)->PublicYouTube {PublicYouTube::new(PublicYouTubeConfig{base_url:server.url.clone(),http:HttpConfig::default()}).unwrap()}
fn player(url:&str,language:&str)->Value {json!({"videoDetails":{"videoId":VIDEO},"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[{"baseUrl":url,"languageCode":language}]}}})}

#[tokio::test]
async fn paid_caption_wire_is_authenticated_timestamped_and_never_uses_public_fallback() {
    let server=Server::start().await;let adapter=paid(&server);
    server.reply(Reply::json(200,json!({"video_id":VIDEO,"language":"tr","transcript":[{"text":"Merhaba","start":1.25,"duration":2.5}],"metadata":{"title":"source title"}})));
    let document=adapter.fetch(VIDEO).await.unwrap();
    assert_eq!(document.cues,vec![CaptionCue{text:"Merhaba".into(),start:1.25,duration:2.5}]);
    assert_eq!(document.metadata["metadata"]["title"],"source title");
    let sent=&server.requests()[0];assert_eq!((sent.method.as_str(),sent.path.as_str()),("GET","/api/v2/youtube/transcript"));
    assert_eq!(sent.headers["authorization"],"Bearer paid-key");
    let query:std::collections::HashMap<_,_>=url::form_urlencoded::parse(sent.query.as_bytes()).into_owned().collect();
    assert_eq!(query["video_url"],VIDEO);assert_eq!(query["format"],"json");assert_eq!(query["include_timestamp"],"true");assert_eq!(query["send_metadata"],"true");
    for (reply,error) in [
        (Reply::json(404,json!({})),CaptionError::NoCaptions),
        (Reply::json(200,json!({"video_id":VIDEO,"transcript":[]})),CaptionError::NoCaptions),
        (Reply::json(200,json!({"error":"Sign in for age verification"})),CaptionError::AgeRestricted),
        (Reply::json(429,json!({})),CaptionError::RateLimited),
        (Reply::json(503,json!({})),CaptionError::Network),
        (Reply::json(403,json!({})),CaptionError::Unknown),
    ] {
        server.reply(reply);assert_eq!(adapter.fetch(VIDEO).await,Err(error));
    }
    assert_eq!(server.requests().len(),7,"each paid attempt makes one paid request, never an unauthenticated fallback");
}

#[tokio::test]
async fn public_srv3_milliseconds_and_legacy_seconds_are_parsed_by_protocol_not_duration_guess() {
    let server=Server::start().await;let adapter=public(&server);
    server.reply(Reply::json(200,player(&format!("{}/captions",server.url),"tr")));
    server.reply(Reply::text(200,"<timedtext><body><p t=\"1250\" d=\"90\"><s>Merhaba &amp;#39;</s><s>dünya</s></p></body></timedtext>"));
    let document=adapter.fetch(VIDEO,Some("tr")).await.unwrap();
    assert_eq!(document.cues,vec![CaptionCue{text:"Merhaba 'dünya".into(),start:1.25,duration:0.09}]);
    assert_eq!(document.language.as_deref(),Some("tr"));
    let primary=&server.requests()[0];
    assert_eq!(primary.path,"/youtubei/v1/player");assert_eq!(primary.method,"POST");assert!(!primary.headers.contains_key("authorization"));
    let body:Value=serde_json::from_slice(&primary.body).unwrap();assert_eq!(body["context"]["client"]["clientName"],"ANDROID");assert_eq!(body["videoId"],VIDEO);
    server.reply(Reply::json(200,json!({"playabilityStatus":{"status":"OK"}})));
    let web=format!("<script>var ytInitialPlayerResponse = {}; </script>",player(&format!("{}/legacy",server.url),"en"));
    server.reply(Reply::text(200,&web));
    server.reply(Reply::text(200,"<transcript><text start=\"2.5\" dur=\"100\">Long caption</text></transcript>"));
    let legacy=adapter.fetch(VIDEO,None).await.unwrap();
    assert_eq!(legacy.cues[0].duration,100.0);assert_eq!(legacy.cues[0].start,2.5);
    assert_eq!(server.requests()[3].path,"/watch");
    assert!(server.requests().iter().all(|r|!r.headers.contains_key("authorization")),"public protocol has no paid bearer key to leak");
}

#[tokio::test]
async fn public_innertube_status_and_parse_failures_use_one_watch_fallback() {
    for primary in [
        Reply::text(500,"unavailable"),
        Reply::text(403,"denied"),
        Reply::text(429,"slow down"),
        Reply::text(200,"not JSON"),
    ] {
        let server=Server::start().await;let adapter=public(&server);
        server.reply(primary);
        server.reply(Reply::text(200,&format!("<script>var ytInitialPlayerResponse = {}; </script>",player(&format!("{}/captions",server.url),"en"))));
        server.reply(Reply::text(200,"<transcript><text start=\"1.5\" dur=\"2\">Recovered caption</text></transcript>"));
        let document=adapter.fetch(VIDEO,Some("en")).await.unwrap();
        assert_eq!(document.cues,vec![CaptionCue{text:"Recovered caption".into(),start:1.5,duration:2.0}]);
        let requests=server.requests();
        assert_eq!(requests.iter().map(|r|r.path.as_str()).collect::<Vec<_>>(),vec!["/youtubei/v1/player","/watch","/captions"]);
        assert_eq!(requests[1].headers["accept-language"],"en");
        assert!(requests.iter().all(|r|!r.headers.contains_key("authorization")));
        assert_eq!(server.pending(),0);
    }
}

#[tokio::test]
async fn public_innertube_network_timeout_uses_one_watch_fallback() {
    let server=Server::start().await;
    let adapter=PublicYouTube::new(PublicYouTubeConfig{
        base_url:server.url.clone(),
        http:HttpConfig{request_timeout:std::time::Duration::from_secs(1),..HttpConfig::default()},
    }).unwrap();
    server.reply(Reply::json(200,json!({})).delayed(std::time::Duration::from_secs(60)));
    server.reply(Reply::text(200,&format!("<script>var ytInitialPlayerResponse = {}; </script>",player(&format!("{}/captions",server.url),"en"))));
    server.reply(Reply::text(200,"<transcript><text start=\"0\" dur=\"1\">After timeout</text></transcript>"));
    let document=adapter.fetch(VIDEO,None).await.unwrap();
    assert_eq!(document.cues,vec![CaptionCue{text:"After timeout".into(),start:0.0,duration:1.0}]);
    assert_eq!(server.requests().iter().map(|r|r.path.clone()).collect::<Vec<_>>(),vec!["/youtubei/v1/player","/watch","/captions"]);
    assert_eq!(server.pending(),0);
}

#[tokio::test]
async fn public_fallback_preserves_final_watch_and_caption_failures() {
    for (web,error) in [
        (Reply::text(429,"slow down"),CaptionError::RateLimited),
        (Reply::text(503,"unavailable"),CaptionError::Network),
        (Reply::text(200,"<script>var ytInitialPlayerResponse = {bad JSON};</script>"),CaptionError::Unknown),
        (Reply::text(200,"<html>No player</html>"),CaptionError::VideoUnavailable),
        (Reply::text(200,"<script>var ytInitialPlayerResponse = {};</script>"),CaptionError::NoCaptions),
    ] {
        let server=Server::start().await;let adapter=public(&server);
        server.reply(Reply::text(500,"primary unavailable"));server.reply(web);
        assert_eq!(adapter.fetch(VIDEO,None).await,Err(error));
        assert_eq!(server.requests().iter().map(|r|r.path.clone()).collect::<Vec<_>>(),vec!["/youtubei/v1/player","/watch"]);
    }
    for (caption,error) in [
        (Reply::text(503,"unavailable"),CaptionError::Network),
        (Reply::text(200,"not XML"),CaptionError::Unknown),
    ] {
        let server=Server::start().await;let adapter=public(&server);
        server.reply(Reply::text(200,"not JSON"));
        server.reply(Reply::text(200,&format!("<script>var ytInitialPlayerResponse = {}; </script>",player(&format!("{}/captions",server.url),"en"))));
        server.reply(caption);
        assert_eq!(adapter.fetch(VIDEO,None).await,Err(error));
        assert_eq!(server.requests().iter().map(|r|r.path.clone()).collect::<Vec<_>>(),vec!["/youtubei/v1/player","/watch","/captions"]);
    }
}

#[tokio::test]
async fn public_language_missing_rate_limit_and_hostile_track_url_do_not_fall_through_to_paid_or_arbitrary_hosts() {
    let server=Server::start().await;let adapter=public(&server);
    server.reply(Reply::json(200,player(&format!("{}/captions",server.url),"en")));
    assert_eq!(adapter.fetch(VIDEO,Some("tr")).await,Err(CaptionError::LanguageUnavailable));
    server.reply(Reply::json(200,player(&format!("{}/captions",server.url),"en")));
    server.reply(Reply::text(429,"slow down"));assert_eq!(adapter.fetch(VIDEO,None).await,Err(CaptionError::RateLimited));
    for url in ["http://169.254.169.254/latest/meta-data","https://www.youtube.com.evil.test/track","https://youtube.com@evil.test/track"] {
        server.reply(Reply::json(200,player(url,"en")));assert_eq!(adapter.fetch(VIDEO,None).await,Err(CaptionError::Unknown));
    }
    assert_eq!(server.requests().len(),6,"no extra discovery after a selected track fails, or request to an unadvertised language or hostile destination");
}

#[tokio::test]
async fn metadata_preserves_raw_language_precedence_and_provider_duration_without_product_fallbacks() {
    let server=Server::start().await;
    let adapter=YouTubeMetadata::new(YouTubeMetadataConfig{base_url:server.url.clone(),api_key:SecretString::new("metadata-key"),http:HttpConfig::default()}).unwrap();
    server.reply(Reply::json(200,json!({"items":[{"id":VIDEO,"snippet":{"title":"A video","defaultAudioLanguage":"pt-BR","defaultLanguage":"en","thumbnails":{"medium":{"url":"https://image.test/x"}}},"contentDetails":{"duration":"PT1H2M3S"}}]})));
    let metadata=adapter.fetch(VIDEO).await.unwrap();assert_eq!(metadata.language.as_deref(),Some("pt-BR"));assert_eq!(metadata.duration_seconds,Some(3723.0));
    assert_eq!(metadata.raw["snippet"]["thumbnails"]["medium"]["url"],"https://image.test/x");
    server.reply(Reply::json(200,json!({"items":[{"id":VIDEO,"snippet":{"title":"Silent","defaultLanguage":"ja"},"contentDetails":{"duration":"invalid"}}]})));
    let fallback=adapter.fetch(VIDEO).await.unwrap();assert_eq!(fallback.language.as_deref(),Some("ja"));assert_eq!(fallback.duration_seconds,None);
    server.reply(Reply::json(200,json!({"items":[]})));assert_eq!(adapter.fetch(VIDEO).await,Err(CaptionError::VideoUnavailable));
    let sent=&server.requests()[0];assert_eq!(sent.path,"/youtube/v3/videos");
    let query:std::collections::HashMap<_,_>=url::form_urlencoded::parse(sent.query.as_bytes()).into_owned().collect();
    assert_eq!(query["part"],"snippet,contentDetails");assert_eq!(query["key"],"metadata-key");
}
