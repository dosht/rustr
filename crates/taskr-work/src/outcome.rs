//! What a provider observation means for a work: a finished transcript, the
//! provider's own failure, or nothing yet. Pure; no state is touched here.

use media_ports::ProviderPayload;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Transcript(String),
    Failed(String),
    /// Still queued or processing, or a payload that carries no verdict.
    Nothing,
}

/// The digest that identifies a delivered result, taken over the normalized result and not the
/// bytes that carried it. A webhook and a recovery poll of the same result agree, and a different
/// result (another transcript, a failure after a success) cannot be mistaken for a repeat.
pub fn result_digest(outcome: &Outcome) -> Option<String> {
    let (kind, text) = match outcome {
        Outcome::Transcript(text) => ("transcript", text),
        Outcome::Failed(reason) => ("failed", reason),
        Outcome::Nothing => return None,
    };
    Some(format!("{:x}", Sha256::digest(format!("{kind}\n{text}").as_bytes())))
}

pub fn outcome(payload: &ProviderPayload) -> Outcome {
    match payload {
        ProviderPayload::Soniox { reported, transcript } => match reported.status.as_str() {
            "completed" => match transcript {
                Some(t) => {
                    let text = match t.text.as_deref() {
                        Some(text) if !text.trim().is_empty() => text.to_string(),
                        _ => t.tokens.iter().map(|token| token.text.as_str()).collect::<String>(),
                    };
                    let text = text.trim().to_string();
                    if text.is_empty() { Outcome::Failed("empty_transcript".into()) } else { Outcome::Transcript(text) }
                }
                // The completion notice without its transcript is not a result.
                None => Outcome::Nothing,
            },
            "error" => Outcome::Failed(
                reported.error_message.clone().or_else(|| reported.error_type.clone()).unwrap_or_else(|| "provider_error".into()),
            ),
            _ => Outcome::Nothing,
        },
        ProviderPayload::Deepgram(t) => {
            let text = t
                .results
                .channels
                .first()
                .and_then(|channel| channel.alternatives.first())
                .map(|alternative| alternative.transcript.trim().to_string())
                .unwrap_or_default();
            if text.is_empty() { Outcome::Failed("empty_transcript".into()) } else { Outcome::Transcript(text) }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn soniox(status: &str, transcript: Option<serde_json::Value>) -> ProviderPayload {
        ProviderPayload::Soniox {
            reported: serde_json::from_value(json!({"id": "r", "status": status, "error_message": if status == "error" { json!("bad audio") } else { json!(null) }})).unwrap(),
            transcript: transcript.map(|t| serde_json::from_value(t).unwrap()),
        }
    }

    fn deepgram(text: &str) -> ProviderPayload {
        ProviderPayload::Deepgram(
            serde_json::from_value(json!({
                "metadata": {"request_id": "r"},
                "results": {"channels": [{"alternatives": [{"transcript": text}]}]}
            }))
            .unwrap(),
        )
    }

    #[test]
    fn a_completed_soniox_notice_with_its_transcript_is_a_result_and_without_it_is_nothing() {
        let tokens = json!({"id": "r", "tokens": [{"text": "Hello"}, {"text": " world"}]});
        assert_eq!(outcome(&soniox("completed", Some(tokens))), Outcome::Transcript("Hello world".into()));
        assert_eq!(outcome(&soniox("completed", None)), Outcome::Nothing);
        assert_eq!(outcome(&soniox("processing", None)), Outcome::Nothing);
    }

    #[test]
    fn the_transcript_text_wins_over_tokens_and_blank_text_falls_back_to_them() {
        let both = json!({"id": "r", "text": "Whole text", "tokens": [{"text": "x"}]});
        assert_eq!(outcome(&soniox("completed", Some(both))), Outcome::Transcript("Whole text".into()));
        let blank = json!({"id": "r", "text": "  ", "tokens": [{"text": "from "}, {"text": "tokens"}]});
        assert_eq!(outcome(&soniox("completed", Some(blank))), Outcome::Transcript("from tokens".into()));
    }

    #[test]
    fn a_provider_error_carries_its_message_and_silence_is_a_failure_not_an_empty_transcript() {
        assert_eq!(outcome(&soniox("error", None)), Outcome::Failed("bad audio".into()));
        let silent = json!({"id": "r", "tokens": []});
        assert_eq!(outcome(&soniox("completed", Some(silent))), Outcome::Failed("empty_transcript".into()));
        assert_eq!(outcome(&deepgram("  ")), Outcome::Failed("empty_transcript".into()));
        assert_eq!(outcome(&deepgram(" spoken words ")), Outcome::Transcript("spoken words".into()));
    }
}

#[cfg(test)]
mod digest_tests {
    use super::*;

    #[test]
    fn the_same_result_has_the_same_digest_whatever_carried_it_and_another_result_has_another() {
        let text = result_digest(&Outcome::Transcript("hello".into())).unwrap();
        assert_eq!(text, result_digest(&Outcome::Transcript("hello".into())).unwrap());
        assert_ne!(text, result_digest(&Outcome::Transcript("hello!".into())).unwrap());
        let failure = result_digest(&Outcome::Failed("hello".into())).unwrap();
        assert_ne!(text, failure, "a failure with the same words is not the same result as a transcript");
        assert_eq!(result_digest(&Outcome::Nothing), None, "a notice with no verdict carries no result");
        assert_eq!(text.len(), 64, "sha256 as lower case hex");
    }
}
