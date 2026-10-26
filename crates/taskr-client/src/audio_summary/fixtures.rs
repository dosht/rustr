//! Rust presentation fixtures consumed by both bindings. No hand-written web VMs.
use super::*;

fn ready() -> State {
    let mut state = State::new(Some("synthetic-session-not-a-credential".into()));
    state.works.push(AudioWork { id: "11111111-1111-4111-8111-111111111111".into(), name: "Research notes".into(), status: "completed".into(), transcript: Some("A completed synthetic transcript.".into()) });
    state.selected = Some(state.works[0].id.clone());
    state
}
fn generation(status: GenerationStatus, text: &str) -> Generation {
    Generation { generation_id: "22222222-2222-4222-8222-222222222222".into(), execution_id: "33333333-3333-4333-8333-333333333333".into(),
        work_id: "11111111-1111-4111-8111-111111111111".into(), source_revision: 1, status, sequence: 2, text: text.into(),
        durable: status == GenerationStatus::Completed, remote_outcome_unknown: status != GenerationStatus::Completed, error: None }
}
pub fn all() -> Vec<(String, SummaryView)> {
    let ready_state = ready();
    let mut progressive = ready();
    progressive.load_generation(generation(GenerationStatus::Running, "<b>Provisional</b> summary text"));
    progressive.attached = true;
    let mut detached = ready();
    detached.load_generation(generation(GenerationStatus::Running, "Hello"));
    let mut saved = ready();
    saved.load_generation(generation(GenerationStatus::Completed, "Authoritative saved summary."));
    let mut interrupted = ready();
    let mut failed = generation(GenerationStatus::Interrupted, "Retained partial");
    failed.error = Some("Connection interrupted".into());
    interrupted.load_generation(failed);
    vec![("ready".into(), ready_state.view()), ("provisional".into(), progressive.view()), ("detached".into(), detached.view()), ("saved".into(), saved.view()), ("interrupted".into(), interrupted.view())]
}
