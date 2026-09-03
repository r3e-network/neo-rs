//! BIP-39 mnemonic helpers (multi-language wordlists).

use ::bip39::{Language, Mnemonic};
use std::env;
use zeroize::Zeroizing;

const PARSE_LANGUAGES: [Language; 10] = [
    Language::English,
    Language::TraditionalChinese,
    Language::SimplifiedChinese,
    Language::Portuguese,
    Language::Korean,
    Language::Japanese,
    Language::Italian,
    Language::French,
    Language::Spanish,
    Language::Czech,
];

/// Generates a BIP-39 mnemonic from entropy using the current locale (falls back to English).
///
/// # Security
///
/// The returned mnemonic words are private-key-equivalent material.
/// Callers MUST ensure the returned `Vec<String>` is not persisted to disk
/// or logged, and should be dropped as soon as possible. `String` heap
/// allocations are not guaranteed to be zeroed by the allocator on drop;
/// consider converting to a seed immediately and discarding the word list.
pub fn get_mnemonic_code(entropy: &[u8]) -> Result<Vec<String>, String> {
    let language = current_language().unwrap_or_else(|| "en".to_string());
    get_mnemonic_code_with_language(entropy, &language)
}

/// Generates a BIP-39 mnemonic from entropy using the specified language code.
///
/// # Security
///
/// The returned mnemonic words are private-key-equivalent material.
/// See [`get_mnemonic_code`] for security considerations.
pub fn get_mnemonic_code_with_language(
    entropy: &[u8],
    language: &str,
) -> Result<Vec<String>, String> {
    if entropy.len() < 16 || entropy.len() > 32 {
        return Err("The length of entropy should be between 128 and 256 bits.".to_string());
    }
    if entropy.len() % 4 != 0 {
        return Err("The length of entropy should be a multiple of 32 bits.".to_string());
    }

    let mnemonic = Mnemonic::from_entropy_in(resolve_language(language), entropy)
        .map_err(|error| error.to_string())?;
    Ok(mnemonic.words().map(str::to_string).collect())
}

/// Converts a BIP-39 mnemonic back to entropy (any supported language).
///
/// The returned entropy is wrapped in [`Zeroizing`] so the seed material
/// is automatically zeroed when dropped.
pub fn mnemonic_to_entropy(mnemonic: &[&str]) -> Result<Zeroizing<Vec<u8>>, String> {
    let word_count = mnemonic.len();
    if !(12..=24).contains(&word_count) || word_count % 3 != 0 {
        return Err("The number of words should be 12, 15, 18, 21 or 24.".to_string());
    }
    for word in mnemonic {
        if word.trim() != *word || word.split_whitespace().count() != 1 {
            return Err(unknown_word_error(word));
        }
    }

    let phrase = mnemonic.join(" ");
    let parsed = parse_mnemonic_phrase(&phrase)?;

    Ok(Zeroizing::new(parsed.to_entropy()))
}

fn resolve_language(language: &str) -> Language {
    let mut lang = normalize_language_code(language);
    if lang.is_empty() {
        return Language::English;
    }

    loop {
        if let Some(language) = language_from_code(lang.as_str()) {
            return language;
        }
        if let Some(pos) = lang.rfind('-') {
            lang.truncate(pos);
        } else {
            break;
        }
    }

    Language::English
}

fn language_from_code(language: &str) -> Option<Language> {
    match language {
        "cs" => Some(Language::Czech),
        "en" => Some(Language::English),
        "es" => Some(Language::Spanish),
        "fr" => Some(Language::French),
        "it" => Some(Language::Italian),
        "ja" => Some(Language::Japanese),
        "ko" => Some(Language::Korean),
        "pt" => Some(Language::Portuguese),
        "zh" | "zh-hans" | "zh-cn" | "zh-sg" => Some(Language::SimplifiedChinese),
        "zh-hant" | "zh-tw" | "zh-hk" | "zh-mo" => Some(Language::TraditionalChinese),
        _ => None,
    }
}

fn parse_mnemonic_phrase(phrase: &str) -> Result<Mnemonic, String> {
    let mut unknown_word = None;
    let mut checksum_mismatch = false;
    for language in PARSE_LANGUAGES {
        match Mnemonic::parse_in(language, phrase) {
            Err(::bip39::Error::InvalidChecksum) => {
                checksum_mismatch = true;
            }
            Err(::bip39::Error::UnknownWord(index)) if unknown_word.is_none() => {
                unknown_word = phrase.split_whitespace().nth(index);
            }
            Err(_) => {}
            Ok(mnemonic) => return Ok(mnemonic),
        }
    }

    if checksum_mismatch {
        return Err("Invalid mnemonic: checksum does not match.".to_string());
    }

    Err(unknown_word_error(unknown_word.unwrap_or_default()))
}

fn unknown_word_error(word: &str) -> String {
    format!("The word '{}' is not in the BIP-39 wordlist.", word)
}

fn normalize_language_code(language: &str) -> String {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut value = trimmed.split('.').next().unwrap_or(trimmed).to_string();
    value = value.split('@').next().unwrap_or(&value).to_string();
    value = value.replace('_', "-");

    let lower = value.to_ascii_lowercase();
    if lower == "c" || lower == "posix" || lower == "iv" {
        return String::new();
    }

    lower
}

fn current_language() -> Option<String> {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = env::var(key) {
            let normalized = normalize_language_code(&value);
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }
    None
}
