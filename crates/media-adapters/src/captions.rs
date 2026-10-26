use media_ports::*;
use reqwest::{Client, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use crate::http::{self, HttpConfig};

#[derive(Clone, Debug)]
pub struct TranscriptApiConfig {
    /// Origin, normally https://transcriptapi.com.
    pub base_url:String,
    pub api_key:SecretString,
    pub http:HttpConfig,
}
pub struct TranscriptApi { client:Client, base:Url, config:TranscriptApiConfig }
#[derive(Clone, Debug)]
pub struct PublicYouTubeConfig {
    /// Origin, normally https://www.youtube.com. No paid service credentials exist here.
    pub base_url:String,
    pub http:HttpConfig,
}
pub struct PublicYouTube { client:Client, base:Url, config:PublicYouTubeConfig }
#[derive(Clone, Debug)]
pub struct YouTubeMetadataConfig {
    /// Origin, normally https://www.googleapis.com.
    pub base_url:String,
    pub api_key:SecretString,
    pub http:HttpConfig,
}
pub struct YouTubeMetadata { client:Client, base:Url, config:YouTubeMetadataConfig }
fn video_id(id:&str) -> Result<(), CaptionError> {
    if id.len() == 11 && id.bytes().all(|b|b.is_ascii_alphanumeric() || b == b'_' || b == b'-') { Ok(()) }
    else { Err(CaptionError::InvalidVideo) }
}
fn classify(status:u16) -> CaptionError {
    match status { 404 => CaptionError::NoCaptions, 429 => CaptionError::RateLimited,
        408 | 500..=599 => CaptionError::Network, _ => CaptionError::Unknown }
}
async fn caption_bytes(response:Response, config:&HttpConfig) -> Result<Vec<u8>,CaptionError> {
    if !response.status().is_success() { return Err(classify(response.status().as_u16())); }
    http::bytes(response,config.max_response_bytes,"captions").await.map_err(|_|CaptionError::Network)
}
fn cues_valid(cues:&[CaptionCue]) -> bool {
    cues.iter().all(|cue| cue.start.is_finite() && cue.duration.is_finite() && cue.start >= 0.0 && cue.duration >= 0.0)
}
impl TranscriptApi {
    pub fn new(config:TranscriptApiConfig) -> Result<Self,TransportError> {
        if config.api_key.expose().is_empty() { return Err(TransportError::Configuration("TranscriptAPI key required".into())); }
        Ok(Self { client:http::client(&config.http)?,base:http::configured_url(&config.base_url)?,config })
    }
}
impl PaidCaptions for TranscriptApi {
    fn fetch<'a>(&'a self,id:&'a str) -> BoxFuture<'a,Result<CaptionDocument,CaptionError>> {
        Box::pin(async move {
            video_id(id)?;
            let mut url = http::endpoint(&self.base,&["api","v2","youtube","transcript"]).map_err(|_|CaptionError::Unknown)?;
            url.query_pairs_mut().append_pair("video_url",id).append_pair("format","json")
                .append_pair("include_timestamp","true").append_pair("send_metadata","true");
            let response = self.client.get(url).bearer_auth(self.config.api_key.expose()).send().await.map_err(|_|CaptionError::Network)?;
            let body = caption_bytes(response,&self.config.http).await?;
            #[derive(Deserialize)]
            struct Answer {
                video_id:Option<String>,language:Option<String>,transcript:Option<Vec<CaptionCue>>,error:Option<String>,
                #[serde(flatten)] metadata:serde_json::Map<String,Value>,
            }
            let answer:Answer = serde_json::from_slice(&body).map_err(|_|CaptionError::Unknown)?;
            if let Some(error) = answer.error {
                let lower = error.to_lowercase();
                return Err(if lower.contains("age") || lower.contains("restricted") || lower.contains("sign in") { CaptionError::AgeRestricted } else { CaptionError::Unknown });
            }
            if answer.video_id.as_deref() != Some(id) { return Err(CaptionError::Unknown); }
            let cues = answer.transcript.ok_or(CaptionError::NoCaptions)?;
            if cues.is_empty() { return Err(CaptionError::NoCaptions); }
            if !cues_valid(&cues) { return Err(CaptionError::Unknown); }
            Ok(CaptionDocument { video_id:id.into(),language:answer.language,cues,metadata:Value::Object(answer.metadata) })
        })
    }
}
impl PublicYouTube {
    pub fn new(config:PublicYouTubeConfig) -> Result<Self,TransportError> {
        Ok(Self { client:http::client(&config.http)?,base:http::configured_url(&config.base_url)?,config })
    }
    async fn player(&self,id:&str) -> Result<Value,CaptionError> {
        let mut url = http::endpoint(&self.base,&["youtubei","v1","player"]).map_err(|_|CaptionError::Unknown)?;
        url.query_pairs_mut().append_pair("prettyPrint","false");
        let response = self.client.post(url).header("user-agent","com.google.android.youtube/20.10.38 (Linux; U; Android 14)")
            .json(&json!({"context":{"client":{"clientName":"ANDROID","clientVersion":"20.10.38"}},"videoId":id}))
            .send().await.map_err(|_|CaptionError::Network)?;
        let body = caption_bytes(response,&self.config.http).await?;
        serde_json::from_slice(&body).map_err(|_|CaptionError::Unknown)
    }
    async fn web_player(&self,id:&str,language:Option<&str>) -> Result<Value,CaptionError> {
        let mut url = http::endpoint(&self.base,&["watch"]).map_err(|_|CaptionError::Unknown)?;
        url.query_pairs_mut().append_pair("v",id);
        let mut request = self.client.get(url).header("user-agent",WEB_USER_AGENT);
        if let Some(language) = language { request = request.header("accept-language",language); }
        let response = request.send().await.map_err(|_|CaptionError::Network)?;
        let bytes = caption_bytes(response,&self.config.http).await?;
        let page = std::str::from_utf8(&bytes).map_err(|_|CaptionError::Unknown)?;
        if page.contains("class=\"g-recaptcha\"") { return Err(CaptionError::RateLimited); }
        let (_,tail) = page.split_once("var ytInitialPlayerResponse = ").ok_or(CaptionError::VideoUnavailable)?;
        // serde's streaming parser handles braces inside JSON strings, unlike substring brace counts.
        serde_json::Deserializer::from_str(tail).into_iter::<Value>().next().ok_or(CaptionError::Unknown)?
            .map_err(|_|CaptionError::Unknown)
    }
    fn track_url(&self,value:&str) -> Result<Url,CaptionError> {
        let url = Url::parse(value).map_err(|_|CaptionError::Unknown)?;
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() { return Err(CaptionError::Unknown); }
        // Configured loopback origin is used only by synthetic HTTP fixtures. Never accept an
        // arbitrary upstream-supplied loopback origin, even when the player itself is a fixture.
        if self.base.scheme() == "http" && url.origin() == self.base.origin() { return Ok(url); }
        let host = url.host_str().ok_or(CaptionError::Unknown)?;
        if url.scheme() != "https" || url.port_or_known_default() != Some(443)
            || !(host == "youtube.com" || host.ends_with(".youtube.com")) { return Err(CaptionError::Unknown); }
        Ok(url)
    }
}
const WEB_USER_AGENT:&str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_4) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/85.0.4183.83 Safari/537.36";
fn tracks(player:&Value) -> Option<&Vec<Value>> {
    player.pointer("/captions/playerCaptionsTracklistRenderer/captionTracks").and_then(Value::as_array).filter(|tracks|!tracks.is_empty())
}
fn parse_caption_xml(xml:&str) -> Result<Vec<CaptionCue>,CaptionError> {
    let document = roxmltree::Document::parse(xml).map_err(|_|CaptionError::Unknown)?;
    let srv3 = document.descendants().any(|node|node.has_tag_name("p") && node.attribute("t").is_some());
    let mut cues = Vec::new();
    for node in document.descendants().filter(|node|node.has_tag_name(if srv3 { "p" } else { "text" })) {
        let number = |key:&str| node.attribute(key).and_then(|v|v.parse::<f64>().ok()).ok_or(CaptionError::Unknown);
        let (start,duration) = if srv3 { (number("t")? / 1000.0,number("d")? / 1000.0) } else { (number("start")?,number("dur")?) };
        let text:String = node.descendants().filter(|child|child.is_text()).filter_map(|child|child.text()).collect();
        let once = html_escape::decode_html_entities(&text);
        let twice = html_escape::decode_html_entities(&once);
        let text = twice.trim().to_owned();
        if !text.is_empty() { cues.push(CaptionCue { text,start,duration }); }
    }
    if cues.is_empty() { return Err(CaptionError::NoCaptions); }
    if !cues_valid(&cues) { return Err(CaptionError::Unknown); }
    Ok(cues)
}
impl PublicCaptions for PublicYouTube {
    fn fetch<'a>(&'a self,id:&'a str,language:Option<&'a str>) -> BoxFuture<'a,Result<CaptionDocument,CaptionError>> {
        Box::pin(async move {
            video_id(id)?;
            // Source-defined discovery: one watch-page attempt if InnerTube fails or has no
            // tracks. Final watch/track failures propagate; neither endpoint is retried.
            let player = match self.player(id).await {
                Ok(primary) if tracks(&primary).is_some() => primary,
                Ok(_) | Err(_) => self.web_player(id,language).await?,
            };
            if let Some(returned) = player.pointer("/videoDetails/videoId").and_then(Value::as_str)
                && returned != id { return Err(CaptionError::VideoUnavailable); }
            let available = tracks(&player).ok_or(CaptionError::NoCaptions)?;
            let track = match language {
                Some(language) => available.iter().find(|track|track.get("languageCode").and_then(Value::as_str) == Some(language)).ok_or(CaptionError::LanguageUnavailable)?,
                None => &available[0],
            };
            let url = self.track_url(track.get("baseUrl").and_then(Value::as_str).ok_or(CaptionError::Unknown)?)?;
            let actual_language = track.get("languageCode").and_then(Value::as_str).map(str::to_owned);
            let mut request = self.client.get(url).header("user-agent",WEB_USER_AGENT);
            if let Some(language) = language { request = request.header("accept-language",language); }
            let response = request.send().await.map_err(|_|CaptionError::Network)?;
            let bytes = caption_bytes(response,&self.config.http).await?;
            let xml = std::str::from_utf8(&bytes).map_err(|_|CaptionError::Unknown)?;
            Ok(CaptionDocument { video_id:id.into(),language:actual_language,cues:parse_caption_xml(xml)?,metadata:Value::Null })
        })
    }
}
impl YouTubeMetadata {
    pub fn new(config:YouTubeMetadataConfig) -> Result<Self,TransportError> {
        if config.api_key.expose().is_empty() { return Err(TransportError::Configuration("YouTube metadata API key required".into())); }
        Ok(Self { client:http::client(&config.http)?,base:http::configured_url(&config.base_url)?,config })
    }
}
fn duration_seconds(iso:&str) -> Option<f64> {
    let time = iso.strip_prefix("PT")?;
    let mut digits = String::new();
    let mut seconds = 0.0;
    let mut last = 0;
    for ch in time.chars() {
        if ch.is_ascii_digit() || ch == '.' { digits.push(ch); continue; }
        let (rank,multiplier) = match ch { 'H'=>(1,3600.0),'M'=>(2,60.0),'S'=>(3,1.0),_=>return None };
        if rank <= last { return None; }
        last = rank;
        seconds += digits.parse::<f64>().ok()? * multiplier;
        digits.clear();
    }
    if !digits.is_empty() || last == 0 || !seconds.is_finite() { None } else { Some(seconds) }
}
impl VideoMetadataSource for YouTubeMetadata {
    fn fetch<'a>(&'a self,id:&'a str) -> BoxFuture<'a,Result<VideoMetadata,CaptionError>> {
        Box::pin(async move {
            video_id(id)?;
            let mut url = http::endpoint(&self.base,&["youtube","v3","videos"]).map_err(|_|CaptionError::Unknown)?;
            url.query_pairs_mut().append_pair("id",id).append_pair("part","snippet,contentDetails").append_pair("key",self.config.api_key.expose());
            let response = self.client.get(url).send().await.map_err(|_|CaptionError::Network)?;
            let bytes = caption_bytes(response,&self.config.http).await?;
            let mut value:Value = serde_json::from_slice(&bytes).map_err(|_|CaptionError::Unknown)?;
            let items = value.get_mut("items").and_then(Value::as_array_mut).ok_or(CaptionError::VideoUnavailable)?;
            if items.is_empty() { return Err(CaptionError::VideoUnavailable); }
            if items.len() != 1 { return Err(CaptionError::Unknown); }
            let raw = items.pop().ok_or(CaptionError::VideoUnavailable)?;
            if raw.get("id").and_then(Value::as_str) != Some(id) { return Err(CaptionError::VideoUnavailable); }
            let title = raw.pointer("/snippet/title").and_then(Value::as_str).ok_or(CaptionError::Unknown)?.to_owned();
            let language = raw.pointer("/snippet/defaultAudioLanguage").and_then(Value::as_str)
                .or_else(||raw.pointer("/snippet/defaultLanguage").and_then(Value::as_str)).map(str::to_owned);
            let duration_seconds = raw.pointer("/contentDetails/duration").and_then(Value::as_str).and_then(duration_seconds);
            Ok(VideoMetadata { video_id:id.into(),title,duration_seconds,language,raw })
        })
    }
}
