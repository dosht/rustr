use super::*;

#[cfg(not(target_arch = "wasm32"))]
impl SummaryController {
    pub(super) async fn receive(&self, epoch: u64, path: &str, on_view: &impl Fn(SummaryView)) -> Result<(), SummaryError> {
        let session = self.api.sessions.load().ok_or_else(|| SummaryError::from(Failure::NoSession))?;
        let url = format!("{}{path}", self.api.base);
        // ApplicationClient reports only the status; the host's published 429 body is fixed.
        let map_error = |error: llm_http::ClientError| match error {
            llm_http::ClientError::Http(429) => SummaryError { code: "watch_limit".into(), message: "Too many live summary views are attached. Close one, then resume.".into() },
            error => SummaryError {
                code: match error { llm_http::ClientError::Http(401) => "unauthenticated", llm_http::ClientError::Http(410) => "erased", llm_http::ClientError::Http(404) => "not_found", _ => "stream" }.into(),
                message: format!("{error}. Resume updates without generating again."),
            },
        };
        let mut stream = self.application.open(&url, Some(&session.access_token)).await.map_err(map_error)?;
        while let Some(frame) = stream.next().await.map_err(map_error)? {
            let terminal = self.apply_frame(epoch, frame)?;
            self.publish(epoch, on_view);
            if terminal { stream.cancel(); return Ok(()); }
        }
        Err(SummaryError { code: "disconnected".into(), message: "Connection interrupted. Resume updates without generating again.".into() })
    }
}

#[cfg(target_arch = "wasm32")]
impl SummaryController {
    /// reqwest's existing wasm transport wraps fetch's ReadableStream. Decode
    /// bounded byte records directly rather than buffering response.text().
    pub(super) async fn receive(&self, epoch: u64, path: &str, on_view: &impl Fn(SummaryView)) -> Result<(), SummaryError> {
        use futures_util::StreamExt;
        let response = self.api.request(Method::GET, path)?.header("accept", "text/event-stream")
            .send().await.map_err(|e| SummaryError { code: "network".into(), message: e.to_string() })?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let error = response.json::<taskr_api::errors::ErrorBody>().await.ok();
            return Err(SummaryError { code: error.as_ref().map(|e| e.error.code.clone()).unwrap_or_else(|| status.to_string()),
                message: error.map(|e| e.error.message).unwrap_or_else(|| format!("Stream request failed ({status}).")) });
        }
        if !response.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(';').next().is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))) {
            return Err(SummaryError { code: "protocol".into(), message: "Expected a summary event stream.".into() });
        }
        let bytes = response.bytes_stream();
        futures_util::pin_mut!(bytes);
        let mut decoder = llm_http::ApplicationDecoder::new(llm_core::Limits::default());
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|e| SummaryError { code: "network".into(), message: e.to_string() })?;
            let mut offset = 0;
            while offset < chunk.len() {
                let (used, frame) = decoder.push(&chunk[offset..]).map_err(|e| SummaryError { code: "protocol".into(), message: format!("Invalid summary stream: {e:?}") })?;
                offset += used;
                if let Some(frame) = frame {
                    let terminal = self.apply_frame(epoch, frame)?;
                    self.publish(epoch, on_view);
                    if terminal { return Ok(()); }
                }
                if used == 0 { return Err(SummaryError { code: "protocol".into(), message: "Summary decoder made no progress.".into() }); }
            }
        }
        decoder.finish().map_err(|e| SummaryError { code: "protocol".into(), message: format!("Incomplete summary stream: {e:?}") })?;
        Err(SummaryError { code: "disconnected".into(), message: "Connection interrupted. Resume updates without generating again.".into() })
    }
}
