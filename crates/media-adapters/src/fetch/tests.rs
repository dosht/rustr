use super::*;
use crate::storage::{S3Store,S3StoreConfig};
use crate::test_support::{Reply,Server};

fn fetcher(server:&Server)->ControlledFetcher {
    let mut fetcher=ControlledFetcher::new(FetchConfig::default()).unwrap();
    fetcher.fixture_address=Some(server.url.trim_start_matches("http://").parse().unwrap());
    fetcher
}
fn source(server:&Server,path:&str)->SensitiveUrl {SensitiveUrl::new(format!("{}{path}",server.url.replace("127.0.0.1","fixture.invalid")))}
fn target()->ObjectRef {ObjectRef{store:StoreKind::R2,bucket:"staging".into(),key:"account-a/job-73/attempt-a.wav".into(),version:None}}
fn storage(server:&Server)->S3Store {S3Store::new(S3StoreConfig{kind:StoreKind::R2,endpoint:server.url.clone(),bucket:"staging".into(),region:"auto".into(),access_key:SecretString::new("fixture"),secret_key:SecretString::new("secret"),http:HttpConfig::default()}).unwrap()}

#[test]
fn private_reserved_and_tunnel_addresses_cannot_be_external_audio_destinations() {
    for address in ["0.1.2.3","10.0.0.1","100.64.0.1","127.0.0.1","169.254.169.254","172.16.0.1","172.31.255.255","192.168.1.1",
        "192.0.0.1","192.0.2.1","198.18.0.1","198.51.100.1","203.0.113.1","224.0.0.1","240.0.0.1","255.255.255.255",
        "::","::1","::ffff:127.0.0.1","::ffff:169.254.169.254","fc00::1","fd00::1","fe80::1","ff02::1","2001:db8::1","2002:7f00:1::","64:ff9b::7f00:1"] {
        assert!(!is_public_address(address.parse().unwrap()),"{address}");
    }
    assert!(is_public_address("8.8.8.8".parse().unwrap()));
    assert!(is_public_address("2606:4700:4700::1111".parse().unwrap()));
}

#[tokio::test]
async fn real_external_body_is_pinned_fetched_spooled_and_uploaded_not_handed_directly_to_provider() {
    let source_server=Server::start().await;let store_server=Server::start().await;
    let fetcher=fetcher(&source_server);let store=storage(&store_server);
    source_server.reply(Reply::text(200,"synthetic-audio-payload").header("content-type","audio/wav"));
    store_server.reply(Reply::text(200,"").header("x-amz-version-id","staged-version"));
    let staged=fetcher.stage(&source(&source_server,"/audio"),&target(),&store).await.unwrap();
    assert_eq!(staged.object.version.as_deref(),Some("staged-version"));assert_eq!(staged.size,23);
    let source_request=&source_server.requests()[0];assert_eq!(source_request.method,"GET");
    assert!(source_request.headers["host"].to_str().unwrap().starts_with("fixture.invalid:"),"DNS pin preserves original Host/TLS authority");
    let uploaded=&store_server.requests()[0];assert_eq!(uploaded.method,"PUT");assert_eq!(uploaded.body,b"synthetic-audio-payload");
    assert_eq!(uploaded.headers["content-length"],"23");assert_eq!(uploaded.headers["content-type"],"audio/wav");
}

#[tokio::test]
async fn direct_private_and_private_redirects_fail_before_target_connect_or_staging_upload() {
    let source_server=Server::start().await;let store_server=Server::start().await;let private_server=Server::start().await;
    let fetcher=fetcher(&source_server);let store=storage(&store_server);
    assert_eq!(fetcher.stage(&SensitiveUrl::new(format!("{}/audio",private_server.url)),&target(),&store).await,Err(TransportError::UnsafeDestination));
    source_server.reply(Reply::text(302,"").header("location",&format!("{}/metadata",private_server.url)));
    assert_eq!(fetcher.stage(&source(&source_server,"/redirect"),&target(),&store).await,Err(TransportError::UnsafeDestination));
    assert!(private_server.requests().is_empty());assert!(store_server.requests().is_empty());
    assert_eq!(source_server.requests().len(),1);
    assert_eq!(fetcher.stage(&SensitiveUrl::new("http://user:pass@fixture.invalid/audio"),&target(),&store).await,Err(TransportError::UnsafeDestination));
}

#[tokio::test]
async fn redirect_limit_is_explicit_and_customer_post_never_forwards_payload_to_redirect_destination() {
    let server=Server::start().await;let destination=Server::start().await;
    let mut fetcher=fetcher(&server);fetcher.config.max_redirects=1;
    server.reply(Reply::text(302,"").header("location","/again"));server.reply(Reply::text(302,"").header("location","/again"));
    assert!(matches!(fetcher.download(&source(&server,"/start")).await,Err(TransportError::LimitExceeded(_))));
    server.reply(Reply::text(307,"").header("location",&destination.url));
    assert_eq!(fetcher.post_json(&source(&server,"/callback"),br#"{"private":"transcript"}"#).await.unwrap(),307);
    assert!(destination.requests().is_empty());assert_eq!(server.requests()[2].body,br#"{"private":"transcript"}"#);
}
