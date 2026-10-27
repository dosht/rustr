//! CLDR cardinal plural categories for whole numbers, the part of
//! `Intl.PluralRules` that i18next's `count` uses for the 16 UI locales
//! (`transgate/web/locales/config.json`). Counts here are always integers
//! (`Arg::Number` is `i64`), so only the integer rules matter.

/// The plural category of `count` in `locale`.
pub fn category(locale: &str, count: i64) -> &'static str {
    let n = count.unsigned_abs();
    let one_if_1 = if n == 1 { "one" } else { "other" };
    // CLDR 42+: whole millions are "many" in the Romance languages.
    let romance_many = n != 0 && n.is_multiple_of(1_000_000);
    match locale {
        "id" => "other",
        "en" | "de" | "sv" | "fi" | "da" | "no" | "el" | "tr" | "az" | "kk" => one_if_1,
        "it" | "es" => {
            if n == 1 {
                "one"
            } else if romance_many {
                "many"
            } else {
                "other"
            }
        }
        "pt" | "fr" => {
            if n <= 1 {
                "one"
            } else if romance_many {
                "many"
            } else {
                "other"
            }
        }
        "ru" => {
            let (m10, m100) = (n % 10, n % 100);
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        _ => "other",
    }
}
