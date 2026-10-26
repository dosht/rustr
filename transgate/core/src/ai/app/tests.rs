use std::collections::VecDeque;
use std::future::Future;
use std::sync::Mutex;

use billing_core::ledger::Package;
use llm_core::{CompletionFinish, CompletionStream, FinishReason};

use super::*;
use crate::ai::ports::{CounterKind, CounterScope};
use crate::ai::presentation::{AnalysisEvent, analysis_events};
use crate::jobs::testing::{MemStore, NoSleep, TestClock};
use crate::presentation::fixtures;

const NOW: Micros = Micros(1_800_000_000_000_000);
const LINE: &str = "We agreed to ship the quarterly report on Friday and review the budget next week.";

/// One scripted provider step: an event, or a side effect on the store between
/// two events (another request committing meanwhile).
enum Step {
    Event(CompletionEvent),
    Meanwhile(Box<dyn FnOnce() + Send>),
    /// The provider stays silent for a while (the read is pending once).
    Quiet,
}

#[derive(Default)]
struct ScriptedLlm {
    scripts: Mutex<VecDeque<Vec<Step>>>,
    opened: Mutex<Vec<CompletionRequest>>,
    /// When set, the next `open` fails with it.
    open_error: Mutex<Option<LlmError>>,
    /// When set, runs as the request is sent; the headers then never arrive.
    headers_never: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl ScriptedLlm {
    fn script(&self, steps: Vec<Step>) {
        self.scripts.lock().unwrap().push_back(steps);
    }
    fn opened(&self) -> usize {
        self.opened.lock().unwrap().len()
    }
}

struct ScriptedStream(VecDeque<Step>);

impl CompletionStream for ScriptedStream {
    fn next(&mut self) -> llm_core::BoxFuture<'_, Result<Option<CompletionEvent>, LlmError>> {
        Box::pin(async move {
            loop {
                match self.0.pop_front() {
                    Some(Step::Meanwhile(f)) => f(),
                    Some(Step::Quiet) => YieldOnce(false).await,
                    Some(Step::Event(e)) => return Ok(Some(e)),
                    None => return Ok(None),
                }
            }
        })
    }
    fn cancel(&mut self) {
        self.0.clear();
    }
}

/// Pending on its first poll (waking at once), ready on the next.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

impl Llm for ScriptedLlm {
    fn open(&self, request: CompletionRequest) -> llm_core::BoxFuture<'_, Result<Box<dyn CompletionStream>, LlmError>> {
        self.opened.lock().unwrap().push(request);
        if let Some(e) = self.open_error.lock().unwrap().take() {
            return Box::pin(async move { Err(e) });
        }
        let steps = self.scripts.lock().unwrap().pop_front().unwrap_or_default();
        let held = self.headers_never.lock().unwrap().take();
        Box::pin(async move {
            if let Some(meanwhile) = held {
                // The request is out; its headers never arrive in this test.
                meanwhile();
                std::future::pending::<()>().await;
            }
            Ok(Box::new(ScriptedStream(steps.into())) as Box<dyn CompletionStream>)
        })
    }
}

fn delta(text: &str) -> Step {
    Step::Event(CompletionEvent::Delta(text.into()))
}

fn stop() -> Step {
    Step::Event(CompletionEvent::Finished(CompletionFinish { reason: FinishReason::Stop, usage: None, remote_id: Some("cmpl-1".into()) }))
}

fn answer(text: &str) -> Vec<Step> {
    vec![delta(text), stop()]
}

struct World {
    store: MemStore,
    llm: Arc<ScriptedLlm>,
    clock: Arc<TestClock>,
}

fn account() -> AccountId {
    fixtures::account()
}

impl World {
    fn new() -> World {
        let w = World { store: MemStore::default(), llm: Arc::new(ScriptedLlm::default()), clock: Arc::new(TestClock(Mutex::new(NOW))) };
        w.store.with(|d| {
            d.accounts.insert(account());
            d.jobs.insert(1, fixtures::job(1, JobStatus::Completed, "a.mp3"));
            d.lines.push(fixtures::line(1, 1, "00:00:00", "00:00:05", LINE, Some("Ada")));
        });
        w
    }

    fn config() -> AiConfig {
        AiConfig {
            features_enabled: true,
            chat_enabled: true,
            chat_validation: true,
            limits: ChatLimits {
                questions_per_transcript: Some(50),
                questions_per_user: Some(500),
                tokens_per_transcript: Some(20_000),
                tokens_per_user: Some(50_000),
                reset_period: crate::ai::ports::Period::Daily,
            },
            model: ModelSettings::source(),
            bounds: Bounds::default(),
        }
    }

    fn ai_with(&self, config: AiConfig) -> Ai {
        let services = AiServices {
            store: Arc::new(self.store.clone()),
            clock: self.clock.clone(),
            llm: Some(self.llm.clone()),
            live: Arc::new(LiveHub::new(64 * 1024, 1024 * 1024)),
            sleeper: Arc::new(NoSleep),
        };
        Ai::new(services, config)
    }

    fn ai(&self) -> Ai {
        self.ai_with(World::config())
    }

    fn paid(&self) {
        self.store.with(|d| {
            d.packages.push(Package {
                id: 9,
                account: account(),
                payment_id: Some(1),
                name: Some("Pro".into()),
                minutes_included: 600,
                start_at: Micros(NOW.0 - MICROS_PER_DAY),
                end_at: Micros(NOW.0 + 30 * MICROS_PER_DAY),
            })
        });
    }

    fn counter(&self, scope: CounterScope, kind: CounterKind) -> Option<i64> {
        self.store.with(|d| d.ai.counters.iter().find(|c| c.key.scope == scope && c.key.kind == kind).map(|c| c.count))
    }

    fn generation(&self, id: GenerationId) -> Generation {
        self.store.with(|d| d.ai.generations.iter().find(|g| g.id == id).unwrap().clone())
    }
}

use crate::ids::MICROS_PER_DAY;

fn run<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

fn key(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn ask(message: &str) -> ChatRequest {
    ChatRequest { message: message.into(), history: vec![] }
}

fn granted(ai: &Ai) {
    run(ai.record_consent(account(), true)).unwrap();
}

#[test]
fn chat_missing_rows_and_invalid_context_keep_distinct_source_errors() {
    let cases = [
        (None, "No transcript content found"),
        (Some(String::from("<b></b>")), "No transcript content available for chat"),
        (Some(format!("{}x", "x ".repeat(16))), "No transcript content available for chat"),
        (Some("🙂".repeat(256_001)), "No transcript content available for chat"),
    ];
    for (content, expected) in cases {
        let w = World::new();
        w.store.with(|d| match content {
            None => d.lines.clear(),
            Some(content) => {
                d.lines[0].line = content;
                d.lines[0].speaker = None;
            }
        });
        let ai = w.ai();
        granted(&ai);
        let error = run(ai.start_chat(account(), JobId(1), key(1), ask("What was discussed?"))).unwrap_err();
        assert_eq!(error.http_status(), 400);
        assert_eq!(error.message(), expected);
        assert_eq!(error, AiError::Invalid(expected.into()));
        assert!(w.store.with(|d| d.ai.generations.is_empty() && d.ai.counters.is_empty() && d.ai.chat.is_empty()),
            "rejected context must not admit, meter, or save a question");
        assert_eq!(w.llm.opened(), 0);
    }
}

#[test]
fn chat_minimum_uses_sanitized_utf16_units_and_accepts_fifty() {
    let w = World::new();
    w.store.with(|d| {
        d.lines[0].line = format!("<b>{}é</b>", "🙂".repeat(24));
        d.lines[0].speaker = None;
    });
    let ai = w.ai();
    granted(&ai);
    assert_eq!(
        run(ai.start_chat(account(), JobId(1), key(1), ask("What was discussed?"))),
        Err(AiError::Invalid("No transcript content available for chat".into())),
        "49 sanitized UTF-16 units reject despite longer raw markup and UTF-8 bytes"
    );
    w.store.with(|d| d.lines[0].line = format!("<b>{}é!</b>", "🙂".repeat(24)));
    w.llm.script(answer("The participants expressed approval."));
    let started = run(ai.start_chat(account(), JobId(1), key(2), ask("What was discussed?"))).unwrap();
    assert!(started.created);
    assert_eq!(run(ai.run(started.generation.id)).unwrap().status, GenerationStatus::Completed);
    assert_eq!(w.llm.opened(), 1);
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(1));
    assert_eq!(run(ai.chat_history(account(), JobId(1))).unwrap().messages.len(), 2);
}

// REPAIR-P5-5, P5 D6: the job's running answer is reported with the owner's original question, so
// another tab reattaches. Reading meters and dispatches nothing; a saved pair is history only; work
// admitted under an earlier consent decision is fenced, so no tab is told to attach to it.
#[test]
fn chat_history_reports_the_running_answer_until_its_pair_is_saved() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    w.llm.script(answer("Friday."));
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("  When is it due?  "))).unwrap();
    let history = run(ai.chat_history(account(), JobId(1))).unwrap();
    assert!(history.messages.is_empty());
    let in_flight = history.in_flight.expect("the running answer is reported");
    assert_eq!(in_flight.question, "  When is it due?  ", "D5: the original accepted text");
    assert_eq!((in_flight.generation.id, in_flight.generation.kind), (started.generation.id, GenerationKind::Chat));
    assert!(!in_flight.generation.status.is_terminal());
    assert_eq!(w.llm.opened(), 0, "reading never dispatches");
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(1), "reading never meters");

    run(ai.run(started.generation.id)).unwrap();
    let history = run(ai.chat_history(account(), JobId(1))).unwrap();
    assert_eq!(history.messages.len(), 2);
    assert!(history.in_flight.is_none(), "a saved pair is history, not in flight");

    let fenced = run(ai.start_chat(account(), JobId(1), key(2), ask("And the budget?"))).unwrap();
    run(ai.record_consent(account(), false)).unwrap();
    granted(&ai);
    assert!(!w.generation(fenced.generation.id).status.is_terminal(), "the worker has not seen the fence yet");
    assert!(run(ai.chat_history(account(), JobId(1))).unwrap().in_flight.is_none(), "admitted under the old decision");
}

// AI-016, AI-021, D2, D5: one freemium question reserves on admission, streams, and completes
// atomically: the original question and the sanitized answer are stored as one pair, and each
// counter holds exactly one settlement of ceil(UTF16(sanitized question + answer)/4).
#[test]
fn a_freemium_answer_saves_the_pair_and_settles_once() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    w.llm.script(answer("Friday, <b>with</b> the budget."));
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("  When is the report due?  "))).unwrap();
    assert!(started.created);
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(1), "reserved at admission");
    let done = run(ai.run(started.generation.id)).unwrap();
    assert_eq!(done.status, GenerationStatus::Completed);
    let history = run(ai.chat_history(account(), JobId(1))).unwrap();
    let pair: Vec<(ChatRole, &str, i64)> = history.messages.iter().map(|m| (m.role, m.content.as_str(), m.order)).collect();
    assert_eq!(pair, vec![(ChatRole::User, "  When is the report due?  ", 1), (ChatRole::Assistant, "Friday, with the budget.", 2)]);
    let expected = quota::estimate_tokens("When is the report due?Friday, with the budget.");
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Tokens), Some(expected));
    assert_eq!(w.counter(CounterScope::User, CounterKind::Questions), Some(1));
    assert_eq!(done.tokens_used, Some(expected));
}

// D1/D2: the same request key replays the same generation without another provider call; the
// same key with another question is a conflict, never a replay.
#[test]
fn a_request_key_replays_and_never_redispatches() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    w.llm.script(answer("Friday."));
    let first = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    run(ai.run(first.generation.id)).unwrap();
    let again = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    assert!(!again.created);
    assert_eq!(again.generation.id, first.generation.id);
    assert_eq!(run(ai.run(again.generation.id)).unwrap().status, GenerationStatus::Completed);
    assert_eq!(w.llm.opened(), 1);
    assert_eq!(run(ai.start_chat(account(), JobId(1), key(1), ask("Who?"))), Err(AiError::Conflict));
    assert_eq!(w.store.with(|d| d.ai.chat.len()), 2, "one pair only");
}

// AI-014, FRE-024: a paid caller skips every quota read, reset, reservation and increment, even
// when a stored counter would refuse; the metered lane (one active generation) does not apply.
#[test]
fn paid_callers_bypass_the_quota_lane() {
    let w = World::new();
    w.paid();
    let ai = w.ai();
    granted(&ai);
    let exhausted = crate::ai::ports::QuotaCounter {
        key: crate::ai::ports::CounterKey { account: account(), scope: CounterScope::Transcript(JobId(1)), kind: CounterKind::Questions, period: crate::ai::ports::Period::Transcript },
        count: 50,
        max_limit: Some(50),
        reset_at: None,
    };
    w.store.with(|d| d.ai.counters.push(exhausted));
    let a = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let b = run(ai.start_chat(account(), JobId(1), key(2), ask("Who?"))).unwrap();
    assert!(a.created && b.created, "no busy refusal for unmetered work");
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(50));
    assert_eq!(w.store.with(|d| d.ai.counters.len()), 1, "nothing created");
    assert!(w.generation(a.generation.id).quota.is_none());
}

// AI-015, D2: a freemium question at the stored cap is refused before anything is admitted; a
// second metered question while one is active is busy, not queued.
#[test]
fn freemium_caps_and_the_single_metered_lane_refuse_before_dispatch() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let first = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    assert_eq!(run(ai.start_chat(account(), JobId(1), key(2), ask("Who?"))), Err(AiError::Busy(first.generation.id)));
    run(ai.cancel(account(), first.generation.id)).unwrap();
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(0), "cancellation released the reservation");
    w.store.with(|d| d.ai.counters.iter_mut().filter(|c| c.key.kind == CounterKind::Questions && c.key.scope == CounterScope::User).for_each(|c| c.count = 500));
    match run(ai.start_chat(account(), JobId(1), key(3), ask("Who?"))) {
        Err(AiError::Quota(q)) => assert_eq!(q.code, "USER_QUESTIONS_LIMIT_EXCEEDED"),
        other => panic!("{other:?}"),
    }
    assert_eq!(w.store.with(|d| d.ai.generations.len()), 1);
    assert_eq!(w.llm.opened(), 0);
}

// D3: a refusal committed while the answer streams fences the worker: nothing is saved, the
// reservation is released, and a regrant does not reauthorize that generation.
#[test]
fn refusal_during_streaming_fences_the_answer() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let store = w.store.clone();
    w.llm.script(vec![
        delta("Friday"),
        Step::Meanwhile(Box::new(move || {
            store.with(|d| {
                let c = d.ai.consents.get(&account()).cloned();
                d.ai.consents.insert(account(), consent::decide(c, account(), false, NOW));
            })
        })),
        stop(),
    ]);
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let ended = run(ai.run(started.generation.id)).unwrap();
    assert_eq!((ended.status, ended.interruption.clone()), (GenerationStatus::Interrupted, Some(Interruption::Revoked)));
    assert!(w.store.with(|d| d.ai.chat.is_empty()));
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(0));
    granted(&ai);
    assert_eq!(run(ai.run(started.generation.id)).unwrap().status, GenerationStatus::Interrupted);
    assert_eq!(w.llm.opened(), 1);
}

// D3, AI-023: refusal keeps completed artifacts (readable again after a regrant); DELETE purges
// every older artifact for good, while an answer created after the regrant stays readable.
#[test]
fn refusal_keeps_artifacts_and_delete_purges_older_epochs() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    w.llm.script(answer("Friday."));
    let old = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    run(ai.run(old.generation.id)).unwrap();
    run(ai.record_consent(account(), false)).unwrap();
    assert_eq!(run(ai.chat_history(account(), JobId(1))), Err(AiError::ConsentRequired));
    granted(&ai);
    assert_eq!(run(ai.chat_history(account(), JobId(1))).unwrap().messages.len(), 2, "refusal is not deletion");
    let view = run(ai.revoke_consent(account())).unwrap();
    assert!(!view.cleanup_pending && !view.has_consented);
    granted(&ai);
    assert!(run(ai.chat_history(account(), JobId(1))).unwrap().messages.is_empty());
    assert_eq!(run(ai.generation(account(), old.generation.id)), Err(AiError::NotFound));
    w.llm.script(answer("Next week."));
    let new = run(ai.start_chat(account(), JobId(1), key(2), ask("Budget?"))).unwrap();
    run(ai.run(new.generation.id)).unwrap();
    assert_eq!(run(ai.chat_history(account(), JobId(1))).unwrap().messages.len(), 2);
    assert!(w.counter(CounterScope::User, CounterKind::Questions).is_some(), "quota counters survive DELETE");
}

// D3: a failed purge leaves the DELETE pending with permission already withdrawn; covered
// artifacts are unreadable even after a regrant until the purge pass completes it.
#[test]
fn a_failed_purge_stays_pending_and_covered_rows_stay_hidden() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    w.llm.script(answer("Friday."));
    let old = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    run(ai.run(old.generation.id)).unwrap();
    w.store.with(|d| d.ai.fail_purge = true);
    assert!(run(ai.revoke_consent(account())).unwrap().cleanup_pending);
    granted(&ai);
    assert!(run(ai.chat_history(account(), JobId(1))).unwrap().messages.is_empty(), "covered rows are hidden before the purge");
    w.store.with(|d| d.ai.fail_purge = false);
    assert_eq!(run(ai.purge_pass()).unwrap().completed, 1);
    assert!(!run(ai.consent(account())).unwrap().cleanup_pending);
    assert!(w.store.with(|d| d.ai.chat.is_empty()));
}

// D2: a staged final whose completing transaction never committed (the worker died) is
// completed by recovery without another provider call, and settles once.
#[test]
fn recovery_completes_a_staged_final_without_the_provider() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let token = Uuid::from_u128(77);
    w.store.with(|d| {
        let g = d.ai.generations.iter_mut().find(|g| g.id == started.generation.id).unwrap();
        generation::claim(g, token, Micros(NOW.0 + 1), NOW).unwrap();
        generation::stage(g, token, FinalContent::Text { text: "Friday.".into() }, NOW).unwrap();
    });
    w.clock.advance_minutes(2);
    let report = run(ai.run_pass()).unwrap();
    assert_eq!(report.completed, 1);
    assert_eq!(w.llm.opened(), 0);
    assert_eq!(w.store.with(|d| d.ai.chat.len()), 2);
    let tokens = quota::estimate_tokens("When?Friday.");
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Tokens), Some(tokens));
    assert_eq!(run(ai.run_pass()).unwrap().examined, 0);
}

// D2: a lost worker without a durable final is interrupted with its remote outcome unknown and
// its reservation released; no success is invented and the provider is not called again.
#[test]
fn a_lost_worker_without_a_final_is_interrupted_not_replayed() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    w.store.with(|d| {
        let g = d.ai.generations.iter_mut().find(|g| g.id == started.generation.id).unwrap();
        generation::claim(g, Uuid::from_u128(77), Micros(NOW.0 + 1), NOW).unwrap();
    });
    w.clock.advance_minutes(2);
    let view = run(ai.run(started.generation.id)).unwrap();
    assert_eq!((view.status, view.interruption, view.remote_outcome_unknown), (GenerationStatus::Interrupted, Some(Interruption::WorkerLost), true));
    assert_eq!(w.counter(CounterScope::Transcript(JobId(1)), CounterKind::Questions), Some(0));
    assert_eq!(w.llm.opened(), 0);
}

// D1/D4: a length stop is not success; an edit of the transcript while streaming fences the
// answer as source changed. Neither saves a pair.
#[test]
fn unsuccessful_finish_and_source_edits_interrupt() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    w.llm.script(vec![delta("Fri"), Step::Event(CompletionEvent::Finished(CompletionFinish { reason: FinishReason::Length, usage: None, remote_id: None }))]);
    let a = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let ended = run(ai.run(a.generation.id)).unwrap();
    assert!(matches!(ended.interruption, Some(Interruption::Unsuccessful { .. })));
    let store = w.store.clone();
    w.llm.script(vec![delta("Friday."), Step::Meanwhile(Box::new(move || store.with(|d| *d.ai.revisions.entry(1).or_insert(0) += 1))), stop()]);
    let b = run(ai.start_chat(account(), JobId(1), key(2), ask("When?"))).unwrap();
    assert_eq!(run(ai.run(b.generation.id)).unwrap().interruption, Some(Interruption::SourceChanged));
    assert!(w.store.with(|d| d.ai.chat.is_empty()));
}

// AI-001, AI-003, D1: analysis checks the source order (consent before transcript before the
// feature flag), then streams typed sections; the shared event mapping yields the source's
// typeDetected, sectionReady per section and complete.
#[test]
fn analysis_streams_typed_sections_in_source_event_order() {
    let w = World::new();
    let off = w.ai_with(AiConfig { features_enabled: false, ..World::config() });
    assert_eq!(run(off.start_analysis(account(), JobId(1), key(1))), Err(AiError::ConsentRequired));
    granted(&off);
    assert_eq!(run(off.start_analysis(account(), JobId(1), key(1))), Err(AiError::Disabled));
    let ai = w.ai();
    w.llm.script(vec![delta(include_str!("../analysis/fixtures/generic.txt")), stop()]);
    let started = run(ai.start_analysis(account(), JobId(1), key(1))).unwrap();
    assert_eq!(run(ai.start_analysis(account(), JobId(1), key(2))), Err(AiError::Busy(started.generation.id)));
    let done = run(ai.run(started.generation.id)).unwrap();
    assert_eq!(done.status, GenerationStatus::Completed);
    let events = analysis_events(None, &done);
    assert!(matches!(events.first(), Some(AnalysisEvent::TypeDetected { .. })), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, AnalysisEvent::SectionReady { section_type: "summary", .. })));
    assert!(matches!(events.last(), Some(AnalysisEvent::Complete { .. })));
    assert_eq!(w.llm.opened.lock().unwrap()[0].response_format, None, "D1: live analysis omits json_object");
    assert_eq!(run(ai.analysis(account(), JobId(1))).unwrap().map(|v| v.id), Some(started.generation.id));
}

// D5, JOB-033, AI-025: a completed job gets a durable summary request; the callback boundary
// waits while it is pending; a summary is saved and readable; denied consent or disabled AI
// release the boundary with an explicit outcome and never touch the job.
#[test]
fn the_completion_summary_releases_callback_ordering_with_an_explicit_outcome() {
    let w = World::new();
    let ai = w.ai();
    let request = |job: i64| {
        let mut uow = run(w.store.begin()).unwrap();
        run(Ai::request_summary(uow.as_mut(), account(), JobId(job), NOW)).unwrap();
        run(uow.commit()).unwrap();
    };
    request(1);
    assert_eq!(run(ai.summary_outcome(JobId(1))).unwrap(), None, "pending: callback assembly waits");
    assert_eq!(run(ai.summary_pass()).unwrap().reconciled, 1);
    assert!(matches!(run(ai.summary_outcome(JobId(1))).unwrap(), Some(SummaryOutcome::Denied { .. })), "no consent");
    granted(&ai);
    w.store.with(|d| {
        d.jobs.insert(2, fixtures::job(2, JobStatus::Completed, "b.mp3"));
        d.lines.push(fixtures::line(2, 2, "00:00:00", "00:00:05", LINE, None));
    });
    request(2);
    w.llm.script(answer("A short summary."));
    assert_eq!(run(ai.summary_pass()).unwrap().completed, 1);
    let saved = run(ai.summary_outcome(JobId(2))).unwrap();
    assert!(matches!(saved, Some(SummaryOutcome::Saved { .. })));
    assert_eq!(run(ai.completion_summary(account(), JobId(2))).unwrap().map(|s| s.text), Some("A short summary.".into()));
    assert_eq!(w.store.with(|d| d.jobs[&2].status), JobStatus::Completed);
    let disabled = w.ai_with(AiConfig { features_enabled: false, ..World::config() });
    w.store.with(|d| d.jobs.insert(3, fixtures::job(3, JobStatus::Completed, "c.mp3")));
    request(3);
    run(disabled.summary_pass()).unwrap();
    assert!(matches!(run(ai.summary_outcome(JobId(3))).unwrap(), Some(SummaryOutcome::Absent { .. })));
}

// D3, API-BIL-011: consent and limits metadata never require prior consent; paid callers read
// no limits; nothing is created by reading.
#[test]
fn consent_and_limit_metadata_need_no_prior_consent() {
    let w = World::new();
    let ai = w.ai();
    assert!(!run(ai.consent(account())).unwrap().has_consented);
    let limits = run(ai.limits(account(), JobId(1))).unwrap();
    assert!(limits.has_limits);
    assert_eq!(limits.questions_per_transcript.map(|u| (u.current, u.max)), Some((0, 50)));
    assert!(w.store.with(|d| d.ai.counters.is_empty()), "a read creates nothing");
    w.paid();
    assert_eq!(run(ai.limits(account(), JobId(1))).unwrap(), LimitsView { enabled: true, has_limits: false, ..LimitsView::default() });
}

// D4: a provider that sends `Hello` and then stays quiet still leaves a durable sanitized
// partial for other replicas and reconnects before the final; the final then replaces it.
#[test]
fn a_quiet_provider_still_checkpoints_the_partial_before_the_final() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let seen = Arc::new(Mutex::new(None));
    let (store, record) = (w.store.clone(), seen.clone());
    w.llm.script(vec![
        delta("Hello <i>there</i> "),
        Step::Quiet,
        Step::Meanwhile(Box::new(move || {
            *record.lock().unwrap() = store.with(|d| d.ai.generations.first().map(|g| g.draft.clone()));
        })),
        delta("world."),
        stop(),
    ]);
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("Greeting?"))).unwrap();
    let done = run(ai.run(started.generation.id)).unwrap();
    assert_eq!(*seen.lock().unwrap(), Some(Draft::Text { text: "Hello there".into() }), "checkpointed while quiet, sanitized");
    assert_eq!(done.final_content, Some(FinalContent::Text { text: "Hello there world.".into() }));
}

// D2 single-bucket invariant: a counter rolls over only at an admission, admission is refused
// while the account has an active metered generation, and limits reads project the new period
// without writing. So an answer admitted before midnight and settled after it lands in its
// admission day's bucket, and the next admission starts the new day from its own reservation.
#[test]
fn an_answer_settled_after_midnight_lands_in_its_admission_day() {
    let w = World::new();
    let midnight = quota::next_reset(crate::ai::ports::Period::Daily, NOW).unwrap();
    *w.clock.0.lock().unwrap() = Micros(midnight.0 - crate::ids::MICROS_PER_MINUTE);
    let ai = w.ai();
    granted(&ai);
    let user_tokens = || w.store.with(|d| d.ai.counters.iter().find(|c| c.key.scope == CounterScope::User && c.key.kind == CounterKind::Tokens).copied().unwrap());
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let reserved = user_tokens();
    assert_eq!((reserved.count, reserved.reset_at), (quota::estimate_tokens("When?"), Some(midnight)));
    w.clock.advance_minutes(2);
    let limits = run(ai.limits(account(), JobId(1))).unwrap();
    assert_eq!(limits.tokens_per_user.map(|u| u.current), Some(0), "the new day reads as zero");
    assert_eq!(user_tokens(), reserved, "a limits read never rolls a bucket owned by active work");
    assert!(matches!(run(ai.start_chat(account(), JobId(1), key(2), ask("Who?"))), Err(AiError::Busy(_))), "no rollover while it is active");
    assert_eq!(user_tokens(), reserved);
    w.llm.script(answer("Friday."));
    assert_eq!(run(ai.run(started.generation.id)).unwrap().status, GenerationStatus::Completed);
    let settled = user_tokens();
    assert_eq!((settled.count, settled.reset_at), (quota::estimate_tokens("When?Friday."), Some(midnight)), "settled into the admission day");
    run(ai.start_chat(account(), JobId(1), key(3), ask("Who?"))).unwrap();
    let next_day = user_tokens();
    assert_eq!((next_day.count, next_day.reset_at), (quota::estimate_tokens("Who?"), quota::next_reset(crate::ai::ports::Period::Daily, Micros(midnight.0 + 1))));
}

// ---- D3/D9: durable, payload-free LLM dispatch intents ---------------------------------------

fn dispatch_of(w: &World, id: GenerationId) -> Option<crate::ai::ports::LlmDispatch> {
    w.store.with(|d| d.ai.dispatches.iter().find(|x| x.generation == id).cloned())
}

fn metadata(remote_id: &str) -> Step {
    Step::Event(CompletionEvent::Metadata { remote_id: Some(remote_id.into()), usage: None })
}

// D3/D9: the intent is durable before any provider I/O, records only the trusted provider and
// model, keeps every distinct observed provider id (request id and completion id), and survives
// a consent DELETE purge and a hard job deletion, which remove the generation and its content.
#[test]
fn a_dispatch_intent_is_durable_before_io_and_survives_purges() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let before_io = Arc::new(Mutex::new(None));
    let (store, seen) = (w.store.clone(), before_io.clone());
    w.llm.script(vec![
        Step::Meanwhile(Box::new(move || *seen.lock().unwrap() = store.with(|d| d.ai.dispatches.first().cloned()))),
        metadata("req-1"),
        delta("Friday."),
        Step::Event(CompletionEvent::Finished(CompletionFinish { reason: FinishReason::Stop, usage: None, remote_id: Some("cmpl-1".into()) })),
    ]);
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When is it due?"))).unwrap();
    run(ai.run(started.generation.id)).unwrap();
    let early = before_io.lock().unwrap().clone().expect("recorded before the provider answered");
    assert_eq!((early.provider.as_str(), early.model.as_str(), early.disposition), ("deepseek", "deepseek-chat", crate::ai::ports::DispatchDisposition::Possible));
    let settled = dispatch_of(&w, started.generation.id).unwrap();
    assert_eq!(settled.remote_ids, vec!["req-1".to_string(), "cmpl-1".to_string()], "distinct ids, first seen first");
    assert!(!serde_json::to_string(&settled).unwrap().contains("When is it due"), "payload-free");
    run(ai.revoke_consent(account())).unwrap();
    let mut uow = run(w.store.begin()).unwrap();
    run(uow.hard_delete_job(JobId(1))).unwrap();
    run(uow.commit()).unwrap();
    assert!(w.store.with(|d| d.ai.generations.is_empty() && d.ai.chat.is_empty()));
    assert_eq!(dispatch_of(&w, started.generation.id), Some(settled), "the intent outlives the purge and the job");
}

// D3: a transport that reports nothing was sent records NotDispatched; that never erases an
// observed id, and an observed id later turns it back into a possible dispatch.
#[test]
fn not_dispatched_is_recorded_only_without_dispatch_evidence() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    *w.llm.open_error.lock().unwrap() = Some(LlmError::new(ErrorKind::Configuration, RemoteOutcome::NotDispatched));
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    run(ai.run(started.generation.id)).unwrap();
    let mut d = dispatch_of(&w, started.generation.id).unwrap();
    assert_eq!(d.disposition, crate::ai::ports::DispatchDisposition::NotDispatched);
    d.settle(crate::ai::ports::DispatchDisposition::Possible, Some("req-9"));
    d.settle(crate::ai::ports::DispatchDisposition::NotDispatched, None);
    assert_eq!((d.disposition, d.remote_ids), (crate::ai::ports::DispatchDisposition::Possible, vec!["req-9".to_string()]));
}

// D3/D9: a provider id observed after the account's local cascade removed the generation and its
// dispatch row is kept on the surviving erasure item (attempts bumped so an older sweep cannot
// overwrite it); nothing local is recreated and the item stays unresolved.
#[test]
fn a_late_provider_id_after_the_cascade_updates_only_the_erasure_item() {
    use crate::privacy::erasure::{ErasureItem, ErasureRecord, ErasureState, ItemState, ItemTarget};
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let generation = started.generation.id;
    let erasure = crate::ids::ErasureId::new();
    let item = uuid::Uuid::from_u128(5);
    let store = w.store.clone();
    w.llm.script(vec![
        delta("Fri"),
        Step::Meanwhile(Box::new(move || {
            store.with(|d| {
                // The erasure captured the intent, then the cascade removed every local AI row.
                d.erasures.insert(erasure, ErasureRecord {
                    id: erasure,
                    account: account(),
                    requested_at: NOW,
                    state: ErasureState::LocalErased,
                    local_erased_at: Some(NOW),
                    identity_deleted_at: None,
                    closed_at: None,
                });
                d.items.push(ErasureItem {
                    id: item,
                    erasure,
                    target: ItemTarget::LlmDispatch { generation: generation.0, provider: "deepseek".into(), remote_ids: vec![], unkept_id_observed: false },
                    state: ItemState::Unresolved { reason: crate::privacy::erasure::LLM_RETENTION_UNVERIFIED.into(), next_at: NOW },
                    attempts: 3,
                });
                d.ai = Default::default();
                d.jobs.clear();
                d.lines.clear();
            })
        })),
        metadata("req-late"),
        stop(),
    ]);
    // The worker ends without a local row to write (gone with the cascade); only state matters.
    let _ = run(ai.run(generation));
    let (items, generations, chat) = w.store.with(|d| (d.items.clone(), d.ai.generations.len(), d.ai.chat.len()));
    assert_eq!((generations, chat), (0, 0), "no local content is recreated");
    let late = items.iter().find(|i| i.id == item).unwrap();
    assert_eq!(late.target, ItemTarget::LlmDispatch { generation: generation.0, provider: "deepseek".into(), remote_ids: vec!["req-late".into(), "cmpl-1".into()], unkept_id_observed: false });
    assert!(matches!(late.state, ItemState::Unresolved { .. }));
    assert!(late.attempts > 3, "attempts advance so a stale sweep cannot overwrite the identity");
}

// ---- D4: bounded attachments and fences while waiting for headers ----------------------------

// D4: live watch attachments are capped per account (4) and per process, refused at once with no
// queue, and released when the attachment is dropped (disconnect). The process cap is shared by
// distinct owners.
#[test]
fn watch_attachments_are_capped_and_released_on_disconnect() {
    let w = World::new();
    let other = AccountId(uuid::Uuid::from_u128(42));
    w.store.with(|d| {
        d.accounts.insert(other);
        let mut job = fixtures::job(2, JobStatus::Completed, "b.mp3");
        job.account = other;
        d.jobs.insert(2, job);
        d.lines.push(fixtures::line(2, 2, "00:00:00", "00:00:05", LINE, None));
    });
    let ai = w.ai_with(AiConfig { bounds: Bounds { watches_per_account: 4, watches_per_process: 5, ..Bounds::default() }, ..World::config() });
    granted(&ai);
    run(ai.record_consent(other, true)).unwrap();
    let mine = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap().generation.id;
    let theirs = run(ai.start_chat(other, JobId(2), key(2), ask("Who?"))).unwrap().generation.id;
    let mut held: Vec<Watch> = (0..4).map(|_| run(ai.watch(account(), mine)).unwrap()).collect();
    assert!(matches!(run(ai.watch(account(), mine)), Err(AiError::Capacity)), "the fifth attachment of an account is refused");
    let first_other = run(ai.watch(other, theirs)).expect("another owner still fits the process cap");
    assert!(matches!(run(ai.watch(other, theirs)), Err(AiError::Capacity)), "the process cap holds across owners");
    held.pop();
    let _again = run(ai.watch(other, theirs)).expect("a disconnect of one owner frees a slot for another");
    drop(first_other);
    assert!(run(ai.watch(account(), mine)).is_ok());
}

/// Polls a future by hand, without a runtime or wall clock, at most `polls` times.
fn poll_ready<T>(future: impl Future<Output = T>, polls: usize) -> Option<T> {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    (0..polls).find_map(|_| match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    })
}

// D4: the 250 ms fence cadence also runs while the provider's headers are pending. A refusal
// committed after the request went out stops the worker as revoked although the headers never
// arrive; nothing waits for them.
#[test]
fn a_refusal_while_waiting_for_headers_stops_the_worker() {
    let w = World::new();
    let ai = w.ai();
    granted(&ai);
    let store = w.store.clone();
    *w.llm.headers_never.lock().unwrap() = Some(Box::new(move || {
        store.with(|d| {
            let c = d.ai.consents.get(&account()).cloned();
            d.ai.consents.insert(account(), consent::decide(c, account(), false, NOW));
        })
    }));
    let started = run(ai.start_chat(account(), JobId(1), key(1), ask("When?"))).unwrap();
    let ended = poll_ready(ai.run(started.generation.id), 64).expect("the worker stops without the headers").unwrap();
    assert_eq!((ended.status, ended.interruption), (GenerationStatus::Interrupted, Some(Interruption::Revoked)));
    assert!(matches!(ended.draft, Draft::Empty), "no content after the refusal");
    assert!(ended.remote_outcome_unknown, "the request may have reached the provider");
    assert_eq!(w.llm.opened(), 1);
}
