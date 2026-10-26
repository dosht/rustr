use transgate_core::jobs::app::{JobResult, ResolveAnswer, StartAnswer, StartMeta};
use transgate_core::jobs::domain::JobStatus;

use super::*;

#[test]
fn init_upload_and_streaming_url_need_a_numeric_job_id_and_a_string_name() {
    assert_eq!(parse_job_file(br#"{"jobId": 7, "fileName": "clip.mp3"}"#), Ok(JobFile { job: JobId(7), file_name: "clip.mp3".into() }));
    assert_eq!(parse_job_file(br#"{"jobId": 7.0, "fileName": "a"}"#).map(|f| f.job), Ok(JobId(7)), "a JSON number is a number however it is written");
    // The source's message quotes the values the way JavaScript prints them.
    assert_eq!(parse_job_file(br#"{"fileName": "clip.mp3"}"#), Err("Invalid parameters jobId: undefined, fileName: clip.mp3".into()));
    assert_eq!(parse_job_file(br#"{"jobId": "7", "fileName": 3}"#), Err("Invalid parameters jobId: 7, fileName: 3".into()));
    assert_eq!(parse_job_file(br#"{"jobId": 1.5, "fileName": "a"}"#), Err("Invalid parameters jobId: 1.5, fileName: a".into()));
    assert_eq!(parse_job_file(b"not json"), Err("Invalid parameters jobId: undefined, fileName: undefined".into()));
}

#[test]
fn youtube_requests_name_the_missing_field_in_the_source_words() {
    assert_eq!(parse_youtube_add(br#"{"youtubeUrl": "https://youtu.be/abc", "language": "Turkish"}"#).map(|a| a.language), Ok(Some("Turkish".into())));
    assert_eq!(parse_youtube_add(br#"{"youtubeUrl": "u"}"#).map(|a| a.language), Ok(None));
    assert_eq!(parse_youtube_add(br#"{"youtubeUrl": ""}"#), Err("youtubeUrl is required".into()));
    assert_eq!(parse_youtube_add(b"{}"), Err("youtubeUrl is required".into()));
    assert_eq!(parse_youtube_metadata(br#"{"videoId": "dQw4w9WgXcQ"}"#), Ok("dQw4w9WgXcQ".into()));
    assert_eq!(parse_youtube_metadata(b"{}"), Err("videoId is required".into()));
    assert_eq!(parse_youtube_resolve(br#"{"jobId": 12}"#), Ok(JobId(12)));
    assert_eq!(parse_youtube_resolve(br#"{"jobId": "12"}"#), Err("jobId (number) is required".into()));
    assert_eq!(parse_youtube_resolve(br#"{"jobId": 0}"#), Err("jobId (number) is required".into()), "the source rejects a falsy id");
}

#[test]
fn the_resolve_answer_uses_the_source_field_names() {
    let done = resolve_response(&ResolveAnswer::Completed { job_id: JobId(3) });
    assert_eq!(done, json!({"status": "completed", "jobId": 3}));
    let failed = resolve_response(&ResolveAnswer::Failed { job_id: JobId(3), code: "no_captions".into(), message: "No captions".into() });
    assert_eq!(failed, json!({"status": "failed", "jobId": 3, "error": {"code": "no_captions", "message": "No captions"}}));
}

#[test]
fn a_job_ids_request_needs_positive_integers_and_never_both_modes() {
    let parsed = parse_transcriptions(br#"{"jobIds": [1, 2], "locale": "tr", "options": {"preferredProvider": "deepgram", "fallbackEnabled": false}}"#).unwrap();
    let Transcriptions::JobIds(request) = parsed else { panic!("job ids mode") };
    assert_eq!(request.job_ids, vec![JobId(1), JobId(2)]);
    assert_eq!(request.locale.as_deref(), Some("tr"));
    assert_eq!(request.preferences, Preferences { preferred: Some(Provider::Deepgram), fallback_enabled: false });

    for (body, message) in [
        (r#"{}"#, "Either jobIds or audioUrls must be provided"),
        (r#"{"jobIds": []}"#, "Either jobIds or audioUrls must be provided"),
        (r#"{"jobIds": [1], "audioUrls": [{"audioUrl": "https://a.test/x.mp3"}]}"#, "Cannot provide both jobIds and audioUrls in the same request"),
        (r#"{"jobIds": [1, 0]}"#, "All jobIds must be positive integers"),
        (r#"{"jobIds": [1.5]}"#, "All jobIds must be positive integers"),
        (r#"{"jobIds": ["1"]}"#, "All jobIds must be positive integers"),
        (r#"{"jobIds": [1], "locale": 5}"#, "locale must be a string"),
        (r#"{"jobIds": [1], "options": 5}"#, "options must be an object"),
        (r#"{"jobIds": [1], "options": {"webhookUrl": 5}}"#, "options.webhookUrl must be a string"),
        (r#"{"jobIds": [1], "options": {"webhookUrl": "nope"}}"#, "options.webhookUrl must be a valid URL"),
    ] {
        assert_eq!(parse_transcriptions(body.as_bytes()), Err(message.into()), "{body}");
    }
    let defaults = parse_transcriptions(br#"{"jobIds": [4]}"#).unwrap();
    let Transcriptions::JobIds(request) = defaults else { panic!("job ids mode") };
    assert_eq!(request.preferences, Preferences::default());
}

#[test]
fn an_audio_urls_request_is_checked_item_by_item() {
    let ok = parse_transcriptions(br#"{"audioUrls": [{"audioUrl": "https://a.test/x.mp3", "callbackUrl": "https://c.test/hook", "language": "English"}]}"#).unwrap();
    let Transcriptions::AudioUrls(request) = ok else { panic!("audio urls mode") };
    assert_eq!(request.items[0].callback_url.as_deref(), Some("https://c.test/hook"));
    assert_eq!(request.items[0].language.as_deref(), Some("English"));

    for (body, message) in [
        (r#"{"audioUrls": [1]}"#, "audioUrls[0] must be an object"),
        (r#"{"audioUrls": [{}]}"#, "audioUrls[0].audioUrl is required and must be a string"),
        (r#"{"audioUrls": [{"audioUrl": "not a url"}]}"#, "audioUrls[0].audioUrl must be a valid URL"),
        (r#"{"audioUrls": [{"audioUrl": "https://a.test/x"}, {"audioUrl": "https://a.test/y", "callbackUrl": 4}]}"#, "audioUrls[1].callbackUrl must be a string"),
        (r#"{"audioUrls": [{"audioUrl": "https://a.test/x", "callbackUrl": "nope"}]}"#, "audioUrls[0].callbackUrl must be a valid URL"),
        (r#"{"audioUrls": [{"audioUrl": "https://a.test/x", "language": 4}]}"#, "audioUrls[0].language must be a string"),
    ] {
        assert_eq!(parse_transcriptions(body.as_bytes()), Err(message.into()), "{body}");
    }
}

#[test]
fn instants_print_as_javascript_iso_strings() {
    assert_eq!(iso8601(Micros(0)), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso8601(Micros(1_700_000_000_123_456)), "2023-11-14T22:13:20.123Z");
    assert_eq!(iso8601(Micros(951_782_400_000_000)), "2000-02-29T00:00:00.000Z", "a leap day");
    assert_eq!(iso8601(Micros(-1_000)), "1969-12-31T23:59:59.999Z");
}

#[test]
fn the_start_answer_has_the_wrapped_shape_the_clients_read() {
    let answer = StartAnswer {
        results: vec![
            JobResult { job_id: JobId(9), status: JobStatus::Pending, estimated_completion: Some(Micros(1_700_000_000_000_000)), error: None },
            JobResult { job_id: JobId(10), status: JobStatus::Failed, estimated_completion: None, error: Some("boom".into()) },
        ],
        meta: StartMeta {
            new_balance: 42,
            is_freemium: false,
            total_jobs: 2,
            successful_jobs: 1,
            failed_jobs: 1,
            queued_jobs: 1,
            estimated_completion: None,
            request_type: "jobIds".into(),
        },
    };
    let body = start_response(&answer, "req-1", Micros(1_700_000_000_000_000), 12, "web");
    assert_eq!(body["success"], json!(true));
    assert_eq!(body["data"]["results"][0], json!({"jobId": 9, "status": "pending", "estimatedCompletionTime": "2023-11-14T22:13:20.000Z"}));
    assert_eq!(body["data"]["results"][1], json!({"jobId": 10, "status": "failed", "error": "boom"}));
    assert_eq!(body["data"]["metadata"]["newBalance"], json!(42));
    assert_eq!(body["data"]["metadata"]["requestType"], json!("jobIds"));
    assert_eq!(body["metadata"], json!({"requestId": "req-1", "timestamp": "2023-11-14T22:13:20.000Z", "processingTime": 12, "clientType": "web"}));
}

#[test]
fn draft_options_tell_auto_detect_from_unchanged() {
    let (ids, options) = parse_draft_options(br#"{"jobIds": [1, 2], "language": null, "translateTo": ["Turkish"], "speakers": true}"#).unwrap();
    assert_eq!(ids, vec![JobId(1), JobId(2)]);
    assert_eq!(options.language, Some(None), "null is auto-detect");
    assert_eq!(options.translate_to, Some(vec![Language::from_name("Turkish").unwrap()]));
    assert_eq!(options.speakers, Some(true));

    let (_, untouched) = parse_draft_options(br#"{"jobIds": [3]}"#).unwrap();
    assert_eq!(untouched, DraftOptions::default(), "an absent field changes nothing");
    let (_, named) = parse_draft_options(br#"{"jobIds": [3], "language": "English"}"#).unwrap();
    assert_eq!(named.language, Some(Some(Language::from_name("English").unwrap())));

    for (body, message) in [
        (r#"{}"#, "jobIds must be a nonempty array"),
        (r#"{"jobIds": [0]}"#, "All jobIds must be positive integers"),
        (r#"{"jobIds": [1], "language": "Klingon"}"#, "Unsupported language: Klingon"),
        (r#"{"jobIds": [1], "language": 5}"#, "language must be a string or null"),
        (r#"{"jobIds": [1], "translateTo": "Turkish"}"#, "translateTo must be an array"),
        (r#"{"jobIds": [1], "translateTo": ["Turkish", 7]}"#, "Unsupported language: 7"),
        (r#"{"jobIds": [1], "speakers": "yes"}"#, "speakers must be a boolean"),
    ] {
        assert_eq!(parse_draft_options(body.as_bytes()), Err(message.into()), "{body}");
    }
}

#[test]
fn language_usage_lists_stored_names_with_counts_most_used_first() {
    let english = Language::from_name("English").unwrap();
    let turkish = Language::from_name("Turkish").unwrap();
    let usage = LanguageUsage { languages: vec![(english, 9), (turkish, 2)], targets: vec![(turkish, 4)] };
    assert_eq!(
        language_usage_response(&usage),
        json!({"languages": [{"language": "English", "count": 9}, {"language": "Turkish", "count": 2}], "targets": [{"language": "Turkish", "count": 4}]})
    );
}
