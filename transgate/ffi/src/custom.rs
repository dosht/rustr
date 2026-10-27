//! Core types UniFFI cannot take as plain remote records (private fields, tuples,
//! arrays, generics, `Cow`, `BTreeMap`, `usize`), each converted to a plain shape
//! at the boundary. Every conversion is lossless and total in the Rust-to-foreign
//! direction; a foreign value that core would not construct (an unknown language,
//! a rating outside 1..=5, a malformed id) is refused, never repaired.
//!
//! The foreign names are the aliases below, so the generated Kotlin and Swift APIs
//! carry `ScopeOptionRecord`, `FadeRecord` and the wire records, not Rust spellings.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use billing_core::decimal::Decimal;
use billing_core::types::{AccountId, Micros};
use transgate_core::ai::analysis::Rating as AnalysisRating;
use transgate_core::api::KeyId;
use transgate_core::api::keys::ExpirationPolicy;
use transgate_core::ids::{JobId, LineId};
use transgate_core::languages::Language;
use transgate_core::presentation::api_keys::{OptionVm, ScopeChoice};
use transgate_core::presentation::msg::{Arg, Msg, Ns};
use uniffi::deps::anyhow::{anyhow, Result as FfiResult};
use uuid::Uuid;

// ---- plain scalars and newtypes ------------------------------------------------------------

uniffi::custom_type!(usize, u64, {
    remote,
    lower: |v| v as u64,
    try_lift: |v| Ok(usize::try_from(v)?),
});

uniffi::custom_type!(JobId, i64, { remote, lower: |v| v.0, try_lift: |v| Ok(JobId(v)) });
uniffi::custom_type!(LineId, i64, { remote, lower: |v| v.0, try_lift: |v| Ok(LineId(v)) });
uniffi::custom_type!(Micros, i64, { remote, lower: |v| v.0, try_lift: |v| Ok(Micros(v)) });
uniffi::custom_type!(AccountId, String, { remote, lower: |v| v.0.to_string(), try_lift: |v| Ok(AccountId(parse_uuid(&v)?)) });
uniffi::custom_type!(KeyId, String, { remote, lower: |v| v.0.to_string(), try_lift: |v| Ok(KeyId(parse_uuid(&v)?)) });

fn parse_uuid(text: &str) -> FfiResult<Uuid> {
    Uuid::parse_str(text).map_err(|e| anyhow!("not a UUID: {e}"))
}

// The display name is the stable wire value of a language.
uniffi::custom_type!(Language, String, {
    remote,
    lower: |v| v.name().to_string(),
    try_lift: |v| Language::from_name(&v).ok_or_else(|| anyhow!("unknown language {v:?}")),
});

// Decimal text, exactly as core formats and parses it.
uniffi::custom_type!(Decimal, String, {
    remote,
    lower: |v| v.to_string(),
    try_lift: |v| Decimal::parse(&v).map_err(|e| anyhow!("not a decimal: {e:?}")),
});

// A star rating (1..=5), validated by core's own constructor.
uniffi::custom_type!(AnalysisRating, f64, {
    remote,
    lower: |v| v.value(),
    try_lift: |v| AnalysisRating::try_from(v).map_err(|e| anyhow!("{e}")),
});

// ---- fixed arrays and tuples ---------------------------------------------------------------

/// `PurchaseVm.hour_presets`.
pub type HourPresets = [i64; 3];

uniffi::custom_type!(HourPresets, Vec<i64>, {
    remote,
    lower: |v| v.to_vec(),
    try_lift: |v| HourPresets::try_from(v).map_err(|v| anyhow!("expected 3 hour presets, got {}", v.len())),
});

/// `TranscriptVm.fades`: a row and the opacity percent it fades to.
pub type Fade = (usize, u8);

#[derive(uniffi::Record)]
pub struct FadeRecord {
    pub row: u64,
    pub opacity_percent: u8,
}

uniffi::custom_type!(Fade, FadeRecord, {
    remote,
    lower: |v| FadeRecord { row: v.0 as u64, opacity_percent: v.1 },
    try_lift: |v| Ok((usize::try_from(v.row)?, v.opacity_percent)),
});

// ---- the generic option row of the key screen ----------------------------------------------

pub type ScopeOption = OptionVm<ScopeChoice>;
pub type PolicyOption = OptionVm<ExpirationPolicy>;

#[derive(uniffi::Record)]
pub struct ScopeOptionRecord {
    pub value: ScopeChoice,
    pub label: Msg,
    pub selected: bool,
}

#[derive(uniffi::Record)]
pub struct PolicyOptionRecord {
    pub value: ExpirationPolicy,
    pub label: Msg,
    pub selected: bool,
}

uniffi::custom_type!(ScopeOption, ScopeOptionRecord, {
    remote,
    lower: |v| ScopeOptionRecord { value: v.value, label: v.label, selected: v.selected },
    try_lift: |v| Ok(OptionVm { value: v.value, label: v.label, selected: v.selected }),
});

uniffi::custom_type!(PolicyOption, PolicyOptionRecord, {
    remote,
    lower: |v| PolicyOptionRecord { value: v.value, label: v.label, selected: v.selected },
    try_lift: |v| Ok(OptionVm { value: v.value, label: v.label, selected: v.selected }),
});

// ---- messages ---------------------------------------------------------------------------------

/// A core [`Msg`] without `Cow` and `BTreeMap`: namespace, key and named arguments.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct MsgWire {
    pub ns: Ns,
    pub key: String,
    pub args: HashMap<String, ArgWire>,
}

#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum ArgWire {
    Number { value: i64 },
    Text { value: String },
    /// Translated by the shell first, then interpolated.
    Nested { message: Box<MsgWire> },
}

fn to_wire(msg: Msg) -> MsgWire {
    MsgWire {
        ns: msg.ns,
        key: msg.key.into_owned(),
        args: msg
            .args
            .into_iter()
            .map(|(name, arg)| {
                let arg = match arg {
                    Arg::Number(value) => ArgWire::Number { value },
                    Arg::Text(value) => ArgWire::Text { value },
                    Arg::Msg(nested) => ArgWire::Nested { message: Box::new(to_wire(*nested)) },
                };
                (name.into_owned(), arg)
            })
            .collect(),
    }
}

fn from_wire(wire: MsgWire) -> Msg {
    Msg {
        ns: wire.ns,
        key: Cow::Owned(wire.key),
        args: wire
            .args
            .into_iter()
            .map(|(name, arg)| {
                let arg = match arg {
                    ArgWire::Number { value } => Arg::Number(value),
                    ArgWire::Text { value } => Arg::Text(value),
                    ArgWire::Nested { message } => Arg::Msg(Box::new(from_wire(*message))),
                };
                (Cow::Owned(name), arg)
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

uniffi::custom_type!(Msg, MsgWire, { remote, lower: |v| to_wire(v), try_lift: |v| Ok(from_wire(v)) });

#[cfg(test)]
mod tests {
    use super::*;
    use transgate_core::presentation::msg::{app, common};

    #[test]
    fn a_message_survives_the_wire_with_nested_arguments() {
        let msg = app("p5.erasure.local_data_erased").count(2).nested("language", common("Turkish")).text("name", "Ada");
        assert_eq!(from_wire(to_wire(msg.clone())), msg);
    }
}
