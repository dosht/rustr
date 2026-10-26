//! Freemium chat limits (AI-014..019, FRE-024, D2), pure.
//!
//! Source semantics kept: the four caps are checked in the source order
//! (questions per transcript, questions per user, tokens per transcript,
//! tokens per user); a question cap refuses when the stored count has reached
//! the STORED maximum; a token cap refuses when the count plus the sanitized
//! question's estimate exceeds the CONFIGURED maximum; settlement adds one
//! question and `ceil(UTF16(sanitized question + sanitized answer) / 4)`.
//!
//! D2 corrections: admission reserves the question and its estimate on every
//! enabled counter, bound to the period it was admitted in; settlement and
//! release touch a counter only while it is still that period; a new timed
//! counter gets its first reset deadline (AI-D09).
//!
//! Single-bucket invariant (no history table): a counter row rolls over only
//! in an admission's `refreshed`, and admission is refused (`Busy`) before any
//! counter is locked while the account has an active metered generation;
//! limits reads project with `current` and never write. Hence a reservation's
//! `period_end` still equals its counter's `reset_at` until that reservation
//! is settled or released, even after the boundary passes, and the next
//! admission rolls over only after it. Persistence must not reset counters on
//! any other path.

use crate::ai::QuotaRefusal;
use crate::ai::ports::{CounterKey, CounterKind, CounterScope, Period, QuotaCounter, QuotaReservation, ReservedBucket};
use crate::ids::{AccountId, JobId, MICROS_PER_DAY, Micros};
use crate::time::{civil_from_days, days_from_civil};

/// The configured caps (AI_CHAT_* variables). `None` is no limit (AI-017).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatLimits {
    pub questions_per_transcript: Option<i64>,
    pub questions_per_user: Option<i64>,
    pub tokens_per_transcript: Option<i64>,
    pub tokens_per_user: Option<i64>,
    /// `AI_CHAT_RESET_PERIOD`; anything unrecognized is daily.
    pub reset_period: Period,
}

impl ChatLimits {
    /// Reads the source variables: missing, blank or unparseable means no
    /// limit; a leading integer parses like JavaScript's `parseInt`.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> ChatLimits {
        let cap = |name: &str| get(name).and_then(|v| parse_int(&v));
        let reset_period = match get("AI_CHAT_RESET_PERIOD").map(|v| v.to_lowercase()).as_deref() {
            Some("weekly") => Period::Weekly,
            Some("monthly") => Period::Monthly,
            Some("transcript") => Period::Transcript,
            _ => Period::Daily,
        };
        ChatLimits {
            questions_per_transcript: cap("AI_CHAT_MAX_QUESTIONS_PER_TRANSCRIPT"),
            questions_per_user: cap("AI_CHAT_MAX_QUESTIONS_PER_USER_DAILY"),
            tokens_per_transcript: cap("AI_CHAT_MAX_TOKENS_PER_TRANSCRIPT"),
            tokens_per_user: cap("AI_CHAT_MAX_TOKENS_PER_USER_DAILY"),
            reset_period,
        }
    }

    /// Source `hasAnyLimits`: a cap counts only when truthy (zero is off).
    pub fn any(&self) -> bool {
        self.caps_in_order().iter().any(|(_, _, max)| max.is_some())
    }

    /// The user counters' period: a configured `transcript` period is checked daily.
    pub fn user_period(&self) -> Period {
        if self.reset_period == Period::Transcript { Period::Daily } else { self.reset_period }
    }

    fn caps_in_order(&self) -> [(bool, CounterKind, Option<i64>); 4] {
        let on = |v: Option<i64>| v.filter(|n| *n != 0);
        [
            (true, CounterKind::Questions, on(self.questions_per_transcript)),
            (false, CounterKind::Questions, on(self.questions_per_user)),
            (true, CounterKind::Tokens, on(self.tokens_per_transcript)),
            (false, CounterKind::Tokens, on(self.tokens_per_user)),
        ]
    }

    /// The enabled caps in check order, with their counter keys and configured maxima.
    pub fn enabled(&self, account: AccountId, job: JobId) -> Vec<(CounterKey, i64)> {
        self.caps_in_order()
            .into_iter()
            .filter_map(|(transcript, kind, max)| {
                let max = max?;
                let (scope, period) = if transcript { (CounterScope::Transcript(job), Period::Transcript) } else { (CounterScope::User, self.user_period()) };
                Some((CounterKey { account, scope, kind, period }, max))
            })
            .collect()
    }
}

fn parse_int(value: &str) -> Option<i64> {
    let t = value.trim();
    if t.is_empty() {
        return None;
    }
    let (sign, digits) = match t.as_bytes()[0] {
        b'-' => (-1, &t[1..]),
        b'+' => (1, &t[1..]),
        _ => (1, t),
    };
    let end = digits.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|n| sign * n)
}

/// Source `estimateTokens`: `ceil(UTF-16 length / 4)`.
pub fn estimate_tokens(text: &str) -> i64 {
    (text.encode_utf16().count() as i64 + 3) / 4
}

/// The end of the period containing `now`: next UTC midnight, next ISO Monday
/// or first of next month; transcript counters never reset.
pub fn next_reset(period: Period, now: Micros) -> Option<Micros> {
    let day = now.0.div_euclid(MICROS_PER_DAY);
    let start = |days: i64| Some(Micros(days * MICROS_PER_DAY));
    match period {
        Period::Daily => start(day + 1),
        Period::Weekly => {
            // 1970-01-01 was a Thursday: Monday-based weekday index.
            let monday_index = (day + 3).rem_euclid(7);
            start(day - monday_index + 7)
        }
        Period::Monthly => {
            let (y, m, _) = civil_from_days(day);
            start(days_from_civil(y, i64::from(m) + 1, 1))
        }
        Period::Transcript => None,
    }
}

/// Source `get_or_create_chat_limit` without its AI-D09 defect: a missing row
/// starts at zero with the configured maximum and its first deadline; a timed
/// row past its deadline restarts at zero with the next one.
pub fn refreshed(existing: Option<QuotaCounter>, key: CounterKey, configured_max: i64, now: Micros) -> QuotaCounter {
    match existing {
        None => QuotaCounter { key, count: 0, max_limit: Some(configured_max), reset_at: next_reset(key.period, now) },
        Some(mut c) => {
            if c.reset_at.is_some_and(|r| now.0 >= r.0) {
                c.count = 0;
                c.reset_at = next_reset(key.period, now);
            }
            c
        }
    }
}

fn refusal(key: CounterKey, counter: &QuotaCounter, max: i64, limits: &ChatLimits) -> QuotaRefusal {
    let period_word = match limits.reset_period {
        Period::Daily => "daily",
        Period::Weekly => "weekly",
        Period::Monthly => "monthly",
        Period::Transcript => "transcript",
    };
    let resets = counter.reset_at.map_or_else(|| "soon".to_string(), |r| crate::time::iso8601(r)[..10].to_string());
    let (code, message) = match (key.scope, key.kind) {
        (CounterScope::Transcript(_), CounterKind::Questions) => {
            ("TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED", format!("You have reached the maximum of {max} questions for this transcript."))
        }
        (CounterScope::User, CounterKind::Questions) => (
            "USER_QUESTIONS_LIMIT_EXCEEDED",
            format!("You have reached your {period_word} limit of {max} questions. Limit resets {resets}."),
        ),
        (CounterScope::Transcript(_), CounterKind::Tokens) => {
            ("TRANSCRIPT_TOKENS_LIMIT_EXCEEDED", format!("This question would exceed the token limit for this transcript ({max} tokens)."))
        }
        (CounterScope::User, CounterKind::Tokens) => (
            "USER_TOKENS_LIMIT_EXCEEDED",
            format!("This question would exceed your {period_word} token limit ({max} tokens). Limit resets {resets}."),
        ),
    };
    QuotaRefusal { code: code.into(), message, current: counter.count, max, reset_at: counter.reset_at, period: key.period }
}

/// The outcome of one metered admission over the locked counters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    /// Every refreshed counter to save (creation and resets persist even on
    /// refusal, as the source's lookup does), with reservations applied when admitted.
    pub counters: Vec<QuotaCounter>,
    pub outcome: Result<QuotaReservation, QuotaRefusal>,
}

/// Checks the enabled caps in source order and, when all pass, reserves one
/// question and `estimate_tokens(sanitized_question)` on each. `existing`
/// holds the locked rows in the same order as `limits.enabled(..)`.
pub fn admit(limits: &ChatLimits, caps: &[(CounterKey, i64)], existing: Vec<Option<QuotaCounter>>, sanitized_question: &str, now: Micros) -> Admission {
    let estimate = estimate_tokens(sanitized_question);
    let mut counters: Vec<QuotaCounter> =
        caps.iter().zip(existing).map(|((key, max), row)| refreshed(row, *key, *max, now)).collect();
    for ((key, max), counter) in caps.iter().zip(&counters) {
        let refused = match key.kind {
            CounterKind::Questions => counter.max_limit.is_some_and(|stored| counter.count >= stored),
            CounterKind::Tokens => counter.count + estimate > *max,
        };
        if refused {
            let r = refusal(*key, counter, *max, limits);
            return Admission { counters, outcome: Err(r) };
        }
    }
    let mut buckets = Vec::with_capacity(counters.len());
    for counter in &mut counters {
        let amount = match counter.key.kind {
            CounterKind::Questions => 1,
            CounterKind::Tokens => estimate,
        };
        counter.count += amount;
        buckets.push(ReservedBucket { key: counter.key, period_end: counter.reset_at, amount });
    }
    Admission { counters, outcome: Ok(QuotaReservation { buckets, question_tokens: estimate }) }
}

/// The actual spend of one successful answer for a bucket's counter kind.
pub fn actual(bucket: &ReservedBucket, sanitized_question: &str, sanitized_answer: &str) -> i64 {
    match bucket.key.kind {
        CounterKind::Questions => 1,
        CounterKind::Tokens => {
            let mut both = String::with_capacity(sanitized_question.len() + sanitized_answer.len());
            both.push_str(sanitized_question);
            both.push_str(sanitized_answer);
            estimate_tokens(&both)
        }
    }
}

/// Replaces the reservation with the actual spend while the counter is still
/// in the admission period; a later period's counter is left untouched.
pub fn settle(counter: QuotaCounter, bucket: &ReservedBucket, actual: i64) -> Option<QuotaCounter> {
    (counter.reset_at == bucket.period_end).then(|| QuotaCounter { count: counter.count + actual - bucket.amount, ..counter })
}

/// Returns an unspent reservation while the counter is still in its period.
pub fn release(counter: QuotaCounter, bucket: &ReservedBucket) -> Option<QuotaCounter> {
    (counter.reset_at == bucket.period_end).then(|| QuotaCounter { count: (counter.count - bucket.amount).max(0), ..counter })
}

/// The usage a limits view shows for a stored row, without writing: a row
/// past its deadline reads as zero in the new period.
pub fn current(row: Option<&QuotaCounter>, key: CounterKey, configured_max: i64, now: Micros) -> QuotaCounter {
    refreshed(row.copied(), key, configured_max, now)
}

#[cfg(test)]
#[path = "quota/tests.rs"]
mod tests;
