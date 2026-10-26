use super::*;
use crate::ids::MICROS_PER_SECOND;

const ACCOUNT: AccountId = AccountId(uuid::Uuid::from_u128(1));
const JOB: JobId = JobId(7);
/// 2027-01-13T10:00:00Z, a Wednesday.
const NOW: Micros = Micros(1_799_834_400 * MICROS_PER_SECOND);

fn limits(qpt: Option<i64>, qpu: Option<i64>, tpt: Option<i64>, tpu: Option<i64>) -> ChatLimits {
    ChatLimits { questions_per_transcript: qpt, questions_per_user: qpu, tokens_per_transcript: tpt, tokens_per_user: tpu, reset_period: Period::Daily }
}

fn row(key: CounterKey, count: i64, max: i64, reset_at: Option<Micros>) -> QuotaCounter {
    QuotaCounter { key, count, max_limit: Some(max), reset_at }
}

fn admit_with(l: &ChatLimits, rows: Vec<Option<QuotaCounter>>, question: &str) -> Admission {
    let caps = l.enabled(ACCOUNT, JOB);
    admit(l, &caps, rows, question, NOW)
}

// FRE-024, AI-017: missing, blank or unparseable variables mean no limit; parseInt keeps a
// leading integer; zero disables a cap; an unknown period is daily and transcript user caps run daily.
#[test]
fn configured_caps_follow_the_source_parser() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    let l = ChatLimits::from_env(env(&[
        ("AI_CHAT_MAX_QUESTIONS_PER_TRANSCRIPT", "50"),
        ("AI_CHAT_MAX_QUESTIONS_PER_USER_DAILY", " "),
        ("AI_CHAT_MAX_TOKENS_PER_TRANSCRIPT", "20000tokens"),
        ("AI_CHAT_MAX_TOKENS_PER_USER_DAILY", "lots"),
        ("AI_CHAT_RESET_PERIOD", "Transcript"),
    ]));
    assert_eq!((l.questions_per_transcript, l.questions_per_user, l.tokens_per_transcript, l.tokens_per_user), (Some(50), None, Some(20000), None));
    assert_eq!((l.reset_period, l.user_period()), (Period::Transcript, Period::Daily));
    assert_eq!(ChatLimits::from_env(env(&[("AI_CHAT_RESET_PERIOD", "hourly")])).reset_period, Period::Daily);
    assert!(!limits(Some(0), None, None, None).any(), "a zero cap is off, like the source's truthiness");
}

// AI-015: the question cap compares the STORED maximum (an imported row keeps its own), the
// token cap the CONFIGURED maximum plus the question estimate; caps are checked in source order.
#[test]
fn admission_keeps_source_precedence_and_maxima() {
    let l = limits(Some(50), Some(500), Some(100), Some(50_000));
    let caps = l.enabled(ACCOUNT, JOB);
    // Imported transcript-question row with a stored max of 3, already at 3: refused first,
    // although the configured cap is 50 and the token cap would refuse too.
    let rows = vec![Some(row(caps[0].0, 3, 3, None)), None, Some(row(caps[2].0, 99, 100, None)), None];
    let refused = admit_with(&l, rows, "Why?").outcome.unwrap_err();
    assert_eq!((refused.code.as_str(), refused.current, refused.max), ("TRANSCRIPT_QUESTIONS_LIMIT_EXCEEDED", 3, 50));
    // 96 + ceil(UTF16("12345678")/4) = 98 <= 100 passes; 97 + 2 = 99 passes; 99 + 2 = 101 refuses.
    let at = |n| admit_with(&l, vec![None, None, Some(row(caps[2].0, n, 100, None)), None], "12345678").outcome;
    assert!(at(97).is_ok());
    assert_eq!(at(99).unwrap_err().code, "TRANSCRIPT_TOKENS_LIMIT_EXCEEDED");
}

// AI-016, D2: admission reserves one question and the sanitized question's estimate on every
// enabled counter; settlement replaces the token reservation with ceil(UTF16(q + answer)/4)
// computed once over the concatenation, not two separately rounded estimates.
#[test]
fn settlement_charges_the_combined_estimate() {
    let l = limits(Some(50), None, Some(1000), None);
    let admission = admit_with(&l, vec![None, None], "abcde");
    let reservation = admission.outcome.unwrap();
    assert_eq!(admission.counters.iter().map(|c| c.count).collect::<Vec<_>>(), vec![1, 2]);
    let tokens = reservation.buckets.iter().find(|b| b.key.kind == CounterKind::Tokens).unwrap();
    // "abcde" + "fgh" = 8 UTF-16 units = 2 tokens, while ceil(5/4) + ceil(3/4) would be 3.
    let spend = actual(tokens, "abcde", "fgh");
    assert_eq!(spend, 2);
    let settled = settle(admission.counters[1], tokens, spend).unwrap();
    assert_eq!(settled.count, 2);
    let emoji = actual(tokens, "😀😀", "");
    assert_eq!(emoji, 1, "UTF-16 units, not bytes or chars");
}

// D2: a reservation belongs to the period it was admitted in. When the user counter rolled
// into a new day before settlement, the late spend never lands in the new day's bucket, and a
// release never subtracts from it.
#[test]
fn late_settlement_never_touches_a_later_period() {
    let l = limits(None, None, None, Some(100));
    let admission = admit_with(&l, vec![None], "a question");
    let bucket = admission.outcome.unwrap().buckets[0];
    assert_eq!(bucket.period_end, next_reset(Period::Daily, NOW));
    let next_day = Micros(bucket.period_end.unwrap().0 + 1);
    let rolled = refreshed(Some(admission.counters[0]), bucket.key, 100, next_day);
    assert_eq!(rolled.count, 0);
    assert_eq!(settle(rolled, &bucket, 40), None);
    assert_eq!(release(rolled, &bucket), None);
    assert_eq!(release(admission.counters[0], &bucket).unwrap().count, 0);
}

// AI-018, AI-D09: a new timed counter starts with its first reset deadline; an existing one
// past its deadline restarts at zero with the next; transcript counters never reset.
#[test]
fn periods_reset_on_utc_day_iso_week_and_calendar_month() {
    let day = 24 * 3600 * MICROS_PER_SECOND;
    let midnight = Micros(NOW.0 - 10 * 3600 * MICROS_PER_SECOND);
    assert_eq!(next_reset(Period::Daily, NOW), Some(Micros(midnight.0 + day)));
    assert_eq!(next_reset(Period::Weekly, NOW), Some(Micros(midnight.0 + 5 * day)), "next Monday 00:00 UTC");
    assert_eq!(crate::time::iso8601(next_reset(Period::Monthly, NOW).unwrap()), "2027-02-01T00:00:00.000Z");
    assert_eq!(next_reset(Period::Transcript, NOW), None);
    let key = CounterKey { account: ACCOUNT, scope: CounterScope::User, kind: CounterKind::Questions, period: Period::Daily };
    let fresh = refreshed(None, key, 500, NOW);
    assert_eq!((fresh.count, fresh.max_limit, fresh.reset_at), (0, Some(500), next_reset(Period::Daily, NOW)));
    let stale = row(key, 499, 500, Some(Micros(NOW.0 - 1)));
    assert_eq!(refreshed(Some(stale), key, 500, NOW).count, 0);
    let transcript = CounterKey { scope: CounterScope::Transcript(JOB), period: Period::Transcript, ..key };
    assert_eq!(refreshed(Some(row(transcript, 49, 50, None)), transcript, 50, Micros(NOW.0 + 400 * day)).count, 49);
}
