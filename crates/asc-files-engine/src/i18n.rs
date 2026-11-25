// SPDX-License-Identifier: MIT
//! Spike shim for the app's catalog lookup.

pub fn tr(message: &str) -> String {
    rust_i18n::t!(message).into_owned()
}

pub(crate) fn join_sentences(first: &str, second: &str) -> String {
    join_sentences_in(&rust_i18n::locale(), first, second)
}

fn join_sentences_in(locale: &str, first: &str, second: &str) -> String {
    if first.ends_with(['。', '！', '？']) {
        format!("{first}{second}")
    } else if first.ends_with(['.', '!', '?']) {
        format!("{first} {second}")
    } else {
        rust_i18n::t!(
            "%{first}. %{second}",
            locale = locale,
            first = first,
            second = second
        )
        .into_owned()
    }
}

const SIZE_UNITS: [&str; 5] = [
    "%{size} B",
    "%{size} kB",
    "%{size} MB",
    "%{size} GB",
    "%{size} TB",
];

/// A size in decimal units, as "1.5 MB", rounded to one decimal; legacy's.
pub(crate) fn file_size(bytes: u64) -> String {
    let (mut value, mut unit) = (bytes as f64, 0);
    if bytes >= 1_000 {
        while value >= 1_000.0 && unit + 1 < SIZE_UNITS.len() {
            value /= 1_000.0;
            unit += 1;
        }
        value = (value * 10.0).round() / 10.0;
        if value >= 1_000.0 && unit + 1 < SIZE_UNITS.len() {
            value /= 1_000.0;
            unit += 1;
        }
    }
    let size = if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    rust_i18n::t!(SIZE_UNITS[unit], size = size).into_owned()
}

pub(crate) fn quoted(name: &str) -> String {
    rust_i18n::t!("“%{name}”", name = name).into_owned()
}

/// Joins already translated items with the language's list separator.
pub(crate) fn list(items: impl IntoIterator<Item = String>) -> String {
    list_in(&rust_i18n::locale(), items)
}

pub(crate) fn count(kind: &str, count: usize) -> String {
    count_u64(kind, count as u64)
}

fn list_in(locale: &str, items: impl IntoIterator<Item = String>) -> String {
    items
        .into_iter()
        .reduce(|first, second| {
            rust_i18n::t!(
                "%{first}, %{second}",
                locale = locale,
                first = first,
                second = second
            )
            .into_owned()
        })
        .unwrap_or_default()
}

pub(crate) fn count_u64(kind: &str, count: u64) -> String {
    count_in(&rust_i18n::locale(), kind, count)
}

fn count_in(locale: &str, kind: &str, count: u64) -> String {
    let category = plural_category(locale, count);
    rust_i18n::t!(
        &format!("counts.{kind}.{category}"),
        locale = locale,
        count = integer_in(locale, count)
    )
    .into_owned()
}

fn plural_category(locale: &str, count: u64) -> &'static str {
    match locale {
        "ja" | "ko" | "vi" | "zh-CN" | "zh-TW" => "other",
        "fr" | "pt-BR" if count <= 1 => "one",
        "ru" if count % 10 == 1 && count % 100 != 11 => "one",
        "ru" if (2..=4).contains(&(count % 10)) && !(12..=14).contains(&(count % 100)) => "few",
        "ru" => "many",
        _ if count == 1 => "one",
        _ => "other",
    }
}

fn integer_in(locale: &str, value: u64) -> String {
    group_digits(locale, &value.to_string())
}

fn group_digits(locale: &str, digits: &str) -> String {
    // Spanish leaves four-digit numbers ungrouped.
    let minimum = if locale == "es" { 5 } else { 4 };
    if digits.len() < minimum {
        return digits.to_owned();
    }
    let separator = group_separator(locale);
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3 * separator.len());
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push_str(separator);
        }
        grouped.push(digit);
    }
    grouped
}

pub(crate) fn detected_locale() -> &'static str {
    let get = |name| std::env::var(name).unwrap_or_default();
    resolve_locale(
        &get("LANGUAGE"),
        &get("LC_ALL"),
        &get("LC_MESSAGES"),
        &get("LANG"),
    )
}

fn group_separator(locale: &str) -> &'static str {
    match locale {
        "en" | "ja" | "ko" | "zh-CN" | "zh-TW" => ",",
        "fr" => "\u{202f}",
        "ru" => "\u{a0}",
        _ => ".",
    }
}

fn resolve_locale(language: &str, lc_all: &str, lc_messages: &str, lang: &str) -> &'static str {
    let messages = [lc_all, lc_messages, lang]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or("C");
    // GNU LANGUAGE is a priority list, but the explicit POSIX locale suppresses it.
    if matches!(messages, "C" | "POSIX") {
        return "en";
    }
    language
        .split(':')
        .chain(std::iter::once(messages))
        .find_map(supported_locale)
        .unwrap_or("en")
}

fn supported_locale(value: &str) -> Option<&'static str> {
    let value = value
        .trim()
        .split(['.', '@'])
        .next()?
        .replace('_', "-")
        .to_ascii_lowercase();
    match value.split('-').next()? {
        "en" | "c" | "posix" => Some("en"),
        "fr" => Some("fr"),
        "de" => Some("de"),
        "es" => Some("es"),
        "ja" => Some("ja"),
        "pt" => Some("pt-BR"),
        "ko" => Some("ko"),
        "vi" => Some("vi"),
        "it" => Some("it"),
        "ru" => Some("ru"),
        "zh" => match value.split('-').nth(1) {
            Some("hant" | "tw" | "hk" | "mo") => Some("zh-TW"),
            _ => Some("zh-CN"),
        },
        _ => None,
    }
}
