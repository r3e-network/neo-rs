//! BIP-39 mnemonic helpers (multi-language wordlists).

use ::bip39::{Language, Mnemonic};
use std::env;
use thiserror::Error;
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

/// Error type for BIP-39 mnemonic operations.
///
/// Replaces the previous `Result<_, String>` returns. The two failure
/// modes are: the supplied entropy has the wrong byte length, and the
/// requested language code does not match any built-in wordlist.
#[derive(Debug, Error)]
pub enum Bip39Error {
    /// The supplied entropy is not a valid BIP-39 length
    /// (128/160/192/224/256 bits).
    #[error("Invalid entropy length: {got} bits (expected 128, 160, 192, 224, or 256)")]
    InvalidEntropyLength {
        /// The actual entropy length, in bits.
        got: usize,
    },

    /// The supplied language code is not a built-in wordlist.
    #[error("Unknown language code: {0}")]
    UnknownLanguage(String),
}

impl From<String> for Bip39Error {
    fn from(message: String) -> Self {
        // Bucket legacy strings into a structured variant.
        if message.starts_with("Invalid entropy length") {
            // Try to extract the length number from the message.
            Self::InvalidEntropyLength { got: 0 }
        } else {
            Self::UnknownLanguage(message)
        }
    }
}

impl From<&str> for Bip39Error {
    fn from(message: &str) -> Self {
        Self::from(message.to_string())
    }
}

impl From<Bip39Error> for String {
    fn from(err: Bip39Error) -> Self {
        err.to_string()
    }
}

/// BIP-39 mnemonic helpers (multi-language wordlists).
pub struct Bip39;

impl Bip39 {
    /// Generates a BIP-39 mnemonic from entropy using the current locale (falls back to English).
    ///
    /// # Security
    ///
    /// The returned mnemonic words are private-key-equivalent material.
    /// Callers MUST ensure the returned `Vec<String>` is not persisted to disk
    /// or logged, and should be dropped as soon as possible. `String` heap
    /// allocations are not guaranteed to be zeroed by the allocator on drop;
    /// consider converting to a seed immediately and discarding the word list.
    pub fn get_mnemonic_code(entropy: &[u8]) -> Result<Vec<String>, Bip39Error> {
        let language = current_language().unwrap_or_else(|| "en".to_string());
        Self::get_mnemonic_code_with_language(entropy, &language)
    }

    /// Generates a BIP-39 mnemonic from entropy using the specified language code.
    ///
    /// # Security
    ///
    /// The returned mnemonic words are private-key-equivalent material.
    /// See [`Self::get_mnemonic_code`] for security considerations.
    pub fn get_mnemonic_code_with_language(
        entropy: &[u8],
        language: &str,
    ) -> Result<Vec<String>, Bip39Error> {
        if entropy.len() < 16 || entropy.len() > 32 {
            return Err(Bip39Error::InvalidEntropyLength {
                got: entropy.len() * 8,
            });
        }
        if entropy.len() % 4 != 0 {
            return Err(Bip39Error::InvalidEntropyLength {
                got: entropy.len() * 8,
            });
        }

        let mnemonic = Mnemonic::from_entropy_in(resolve_language(language), entropy)
            .map_err(|error| Bip39Error::UnknownLanguage(error.to_string()))?;
        Ok(mnemonic.words().map(str::to_string).collect())
    }

    /// Converts a BIP-39 mnemonic back to entropy (any supported language).
    ///
    /// The returned entropy is wrapped in [`Zeroizing`] so the seed material
    /// is automatically zeroed when dropped.
    pub fn mnemonic_to_entropy(mnemonic: &[&str]) -> Result<Zeroizing<Vec<u8>>, Bip39Error> {
        let word_count = mnemonic.len();
        if !(12..=24).contains(&word_count) || word_count % 3 != 0 {
            return Err(Bip39Error::UnknownLanguage(format!(
                "The number of words should be 12, 15, 18, 21 or 24 (got {word_count})."
            )));
        }
        for word in mnemonic {
            if word.trim() != *word || word.split_whitespace().count() != 1 {
                return Err(Bip39Error::UnknownLanguage(unknown_word_error(word)));
            }
        }

        let phrase = mnemonic.join(" ");
        let parsed = parse_mnemonic_phrase(&phrase)?;

        Ok(Zeroizing::new(parsed.to_entropy()))
    }
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

fn parse_mnemonic_phrase(phrase: &str) -> Result<Mnemonic, Bip39Error> {
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
        return Err(Bip39Error::UnknownLanguage(
            "Invalid mnemonic: checksum does not match.".to_string(),
        ));
    }

    Err(Bip39Error::UnknownLanguage(unknown_word_error(
        unknown_word.unwrap_or_default(),
    )))
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
