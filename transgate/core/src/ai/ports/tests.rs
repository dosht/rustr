use super::*;

fn dispatch(remote_ids: Vec<String>) -> LlmDispatch {
    LlmDispatch {
        account: AccountId(uuid::Uuid::from_u128(1)),
        generation: GenerationId(uuid::Uuid::from_u128(2)),
        provider: "deepseek".into(),
        model: "deepseek-chat".into(),
        disposition: DispatchDisposition::Possible,
        remote_ids,
        unkept_id_observed: false,
        created_at: Micros(0),
    }
}

// D9: an observed provider id is dispatch evidence even when it is not kept (over 256 bytes, or
// beyond the eight-id bound): a contradictory NotDispatched never turns such a row absent, in the
// same settlement or in any later one, and nothing is fabricated or truncated to remember it.
#[test]
fn an_unkept_observed_id_still_proves_a_possible_dispatch() {
    let overlong = "x".repeat(MAX_DISPATCH_REMOTE_ID_BYTES + 1);
    let mut empty = dispatch(vec![]);
    empty.settle(DispatchDisposition::NotDispatched, Some(&overlong));
    assert_eq!((empty.disposition, empty.remote_ids.len()), (DispatchDisposition::Possible, 0));
    empty.settle(DispatchDisposition::NotDispatched, None);
    assert_eq!((empty.disposition, empty.remote_ids.len()), (DispatchDisposition::Possible, 0), "a later settlement cannot forget it");
    let mut full = dispatch((0..MAX_DISPATCH_REMOTE_IDS).map(|i| format!("id-{i}")).collect());
    full.settle(DispatchDisposition::NotDispatched, Some("ninth"));
    assert_eq!((full.disposition, full.remote_ids.len()), (DispatchDisposition::Possible, MAX_DISPATCH_REMOTE_IDS));
    let mut nothing = dispatch(vec![]);
    nothing.settle(DispatchDisposition::NotDispatched, None);
    assert_eq!(nothing.disposition, DispatchDisposition::NotDispatched, "no evidence at all");
}
