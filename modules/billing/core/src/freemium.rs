//! Freemium: the three coexisting definitions, the web start guard, the
//! transcript display cuts and the chat-limit entitlement.
//!
//! The definitions disagree on purpose (DEF-019): this module keeps each one
//! under its own name instead of unifying them. Lists of locales and
//! languages are host data; this crate only compares against them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::charge::{FREEMIUM_NAME, freemium_only, is_paid_name};
use crate::ledger::{Package, PackageBalance, is_open};
use crate::types::Micros;

/// Web batch limits in minutes, both inclusive (FRE-015).
pub const FREE_LIMIT_MINUTES: i64 = 20;
pub const EXCEPTION_LIMIT_MINUTES: i64 = 45;
pub const DEFAULT_BANNER_MAX: i64 = 20;

/// Display cuts in seconds (FRE-020, FRE-021).
const DISPLAY_CUT_SECONDS: f64 = 1320.0;
const LOAD_CUT_SECONDS: f64 = 1260.0;
const PREVIEW_LINES: usize = 3;
const FADE_OPACITIES: [u8; 3] = [50, 30, 10];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreemiumLists {
    ui_locales: BTreeSet<String>,
    languages: BTreeSet<String>,
}

impl FreemiumLists {
    /// Locales compare exactly as given; language names are lower case.
    pub fn new(ui_locales: &[&str], languages: &[&str]) -> FreemiumLists {
        FreemiumLists {
            ui_locales: ui_locales.iter().map(|s| s.to_string()).collect(),
            languages: languages.iter().map(|s| s.to_lowercase()).collect(),
        }
    }

    pub fn locale_is_exception(&self, locale: &str) -> bool {
        self.ui_locales.contains(locale)
    }

    pub fn language_is_exception(&self, language: &str) -> bool {
        self.languages.contains(&language.to_lowercase())
    }

    pub fn ui_locales(&self) -> &BTreeSet<String> {
        &self.ui_locales
    }

    pub fn languages(&self) -> &BTreeSet<String> {
        &self.languages
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: i64,
    pub duration: Option<i64>,
    pub language: Option<String>,
    pub first_translation_target: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardRefusal {
    pub total_minutes: i64,
}

impl GuardRefusal {
    pub fn message(&self) -> &'static str {
        "Not enough balance."
    }
}

/// The web 20/45-minute guard for a freemium (app definition 1) batch.
///
/// Only the draft with the lowest id supplies language and translation
/// target, as the legacy repository ordered drafts by id; the order of the
/// request array is irrelevant.
pub fn web_guard(drafts: &[Draft], ui_locale: &str, lists: &FreemiumLists) -> Result<(), GuardRefusal> {
    let total: i64 = drafts.iter().filter_map(|d| d.duration).sum();
    if total <= FREE_LIMIT_MINUTES {
        return Ok(());
    }
    let first = drafts.iter().min_by_key(|d| d.id);
    let exception = lists.locale_is_exception(ui_locale)
        || first.is_some_and(|d| {
            d.language.as_deref().is_some_and(|l| lists.language_is_exception(l))
                || d.first_translation_target.as_deref().is_some_and(|l| lists.language_is_exception(l))
        });
    if exception && total <= EXCEPTION_LIMIT_MINUTES {
        Ok(())
    } else {
        Err(GuardRefusal { total_minutes: total })
    }
}

/// App definition 1: exactly one package row ever, and it is the free one.
pub fn is_freemium_app1(packages: &[Package]) -> bool {
    matches!(packages, [only] if only.name.as_deref() == Some(FREEMIUM_NAME))
}

/// App definition 2: no non-free package with `end_at >= now` (start is not
/// checked). `None` is a failed package query, which counts as freemium.
pub fn is_freemium_app2(packages: Option<&[Package]>, now: Micros) -> bool {
    match packages {
        None => true,
        Some(all) => !all.iter().any(|p| is_paid_name(&p.name) && p.end_at >= now),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facets {
    pub database_no_open_paid_package: bool,
    pub historical_single_free_package: bool,
    pub no_unexpired_paid_package: bool,
}

pub fn facets(all: &[Package], now: Micros) -> Facets {
    let open: Vec<PackageBalance> = all
        .iter()
        .filter(|p| is_open(p, now))
        .map(|p| PackageBalance { package: p.clone(), consumed: 0 })
        .collect();
    Facets {
        database_no_open_paid_package: freemium_only(&open),
        historical_single_free_package: is_freemium_app1(all),
        no_unexpired_paid_package: is_freemium_app2(Some(all), now),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DisplayCut {
    /// How many leading lines are shown.
    pub visible: usize,
    /// `(line index, opacity percent)` for the last three visible lines.
    pub fades: Vec<(usize, u8)>,
    pub show_upgrade_banner: bool,
}

/// FRE-020: a freemium viewer of a job over 20 minutes sees lines that start at
/// or before 22:00. Display only; the data was already fetched.
pub fn display_cut(is_freemium: bool, job_minutes: i64, line_starts: &[f64]) -> DisplayCut {
    if !is_freemium || job_minutes <= FREE_LIMIT_MINUTES {
        return DisplayCut { visible: line_starts.len(), fades: vec![], show_upgrade_banner: false };
    }
    let visible = line_starts.iter().filter(|s| **s <= DISPLAY_CUT_SECONDS).count();
    let fade_count = visible.min(FADE_OPACITIES.len());
    let fades = (0..fade_count)
        .map(|i| {
            let index = visible - fade_count + i;
            // Last line gets the lowest opacity.
            let opacity = FADE_OPACITIES[FADE_OPACITIES.len() - fade_count + i];
            (index, opacity)
        })
        .collect();
    DisplayCut { visible, fades, show_upgrade_banner: true }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadCut {
    pub keep: usize,
    pub stop_live: bool,
}

/// FRE-021: loaded lines are cut to 21:00 plus three preview lines, and live
/// updates stop when the last loaded line starts after 21:00.
pub fn load_cut(is_freemium: bool, line_starts: &[f64]) -> LoadCut {
    if !is_freemium {
        return LoadCut { keep: line_starts.len(), stop_live: false };
    }
    let within = line_starts.iter().filter(|s| **s <= LOAD_CUT_SECONDS).count();
    LoadCut {
        keep: (within + PREVIEW_LINES).min(line_starts.len()),
        stop_live: line_starts.last().is_some_and(|s| *s > LOAD_CUT_SECONDS),
    }
}

pub fn banner_progress(current: i64, max: i64) -> f64 {
    if max <= 0 {
        return 100.0;
    }
    (current as f64 / max as f64 * 100.0).min(100.0)
}

/// Configured chat caps; a missing or unparseable value means no limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCaps {
    pub per_transcript_questions: Option<u64>,
    pub per_transcript_tokens: Option<u64>,
    pub per_user_daily_questions: Option<u64>,
    pub per_user_daily_tokens: Option<u64>,
}

impl ChatCaps {
    pub fn parse(
        per_transcript_questions: Option<&str>,
        per_transcript_tokens: Option<&str>,
        per_user_daily_questions: Option<&str>,
        per_user_daily_tokens: Option<&str>,
    ) -> ChatCaps {
        let cap = |v: Option<&str>| v.and_then(|s| s.trim().parse::<u64>().ok());
        ChatCaps {
            per_transcript_questions: cap(per_transcript_questions),
            per_transcript_tokens: cap(per_transcript_tokens),
            per_user_daily_questions: cap(per_user_daily_questions),
            per_user_daily_tokens: cap(per_user_daily_tokens),
        }
    }
}

#[cfg(test)]
mod tests;
