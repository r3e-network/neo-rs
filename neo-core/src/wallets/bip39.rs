//! BIP-39 mnemonic helpers (multi-language wordlists).

use crate::cryptography::Crypto;
use std::collections::HashMap;
use std::env;
use std::sync::LazyLock;
use zeroize::Zeroizing;

// English last so reverse index prefers English for overlapping words.
const WORDLIST_KEYS: [&str; 10] = [
    "cs", "es", "fr", "it", "ja", "ko", "pt", "zh", "zh-hant", "en",
];

static WORDLISTS: LazyLock<HashMap<&'static str, Vec<&'static str>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    map.insert("cs", load_wordlist(include_str!("bip39_wordlists/cs.txt")));
    map.insert("en", load_wordlist(include_str!("bip39_wordlists/en.txt")));
    map.insert("es", load_wordlist(include_str!("bip39_wordlists/es.txt")));
    map.insert("fr", load_wordlist(include_str!("bip39_wordlists/fr.txt")));
    map.insert("it", load_wordlist(include_str!("bip39_wordlists/it.txt")));
    map.insert("ja", load_wordlist(include_str!("bip39_wordlists/ja.txt")));
    map.insert("ko", load_wordlist(include_str!("bip39_wordlists/ko.txt")));
    map.insert("pt", load_wordlist(include_str!("bip39_wordlists/pt.txt")));
    map.insert("zh", load_wordlist(include_str!("bip39_wordlists/zh.txt")));
    map.insert(
        "zh-hant",
        load_wordlist(include_str!("bip39_wordlists/zh-Hant.txt")),
    );
    map
});

static WORDLIST_REVERSE_INDEX: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for key in WORDLIST_KEYS {
        if let Some(wordlist) = WORDLISTS.get(key) {
            for (i, word) in wordlist.iter().enumerate() {
                map.insert(*word, i);
            }
        }
    }
    map
});

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

    let wordlist = resolve_wordlist(language);
    let entropy_bits = entropy.len() * 8;
    let checksum_bits = entropy_bits / 32;
    let total_bits = entropy_bits + checksum_bits;
    let word_count = total_bits / 11;
    let checksum = Crypto::sha256(entropy);

    let mut mnemonic = Vec::with_capacity(word_count);
    for i in 0..word_count {
        let mut index = 0u16;
        for j in 0..11 {
            let bit_pos = i * 11 + j;
            let bit = if bit_pos < entropy_bits {
                get_bit_msb(entropy, bit_pos)
            } else {
                get_bit_msb(&checksum, bit_pos - entropy_bits)
            };
            if bit {
                index |= 1u16 << (10 - j) as u16;
            }
        }
        let word = wordlist
            .get(index as usize)
            .ok_or_else(|| "Mnemonic index out of range".to_string())?;
        mnemonic.push((*word).to_string());
    }

    Ok(mnemonic)
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

    let total_bits = word_count * 11;
    let entropy_bits = total_bits * 32 / 33;
    let checksum_bits = total_bits - entropy_bits;
    let entropy_bytes = entropy_bits / 8;
    let checksum_bytes = checksum_bits.div_ceil(8);

    let mut entropy = vec![0u8; entropy_bytes];
    let mut checksum = vec![0u8; checksum_bytes];

    for (i, word) in mnemonic.iter().enumerate() {
        let index = *WORDLIST_REVERSE_INDEX
            .get(*word)
            .ok_or_else(|| format!("The word '{}' is not in the BIP-39 wordlist.", word))?;
        for j in 0..11 {
            let bit_pos = i * 11 + j;
            let bit = (index & (1 << (10 - j))) != 0;
            if bit_pos < entropy_bits {
                let byte_index = bit_pos / 8;
                let bit_in_byte = 7 - (bit_pos % 8);
                if bit {
                    entropy[byte_index] |= 1 << bit_in_byte;
                }
            } else {
                let cs_bit_pos = bit_pos - entropy_bits;
                let byte_index = cs_bit_pos / 8;
                let bit_in_byte = 7 - (cs_bit_pos % 8);
                if bit {
                    checksum[byte_index] |= 1 << bit_in_byte;
                }
            }
        }
    }

    let hash = Crypto::sha256(&entropy);
    for i in 0..checksum_bits {
        let byte_index = i / 8;
        let bit_in_byte = 7 - (i % 8);
        let bit_from_hash = (hash[byte_index] & (1 << bit_in_byte)) != 0;
        let bit_from_checksum = (checksum[byte_index] & (1 << bit_in_byte)) != 0;
        if bit_from_hash != bit_from_checksum {
            return Err("Invalid mnemonic: checksum does not match.".to_string());
        }
    }

    Ok(Zeroizing::new(entropy))
}

fn load_wordlist(data: &'static str) -> Vec<&'static str> {
    data.lines().filter(|line| !line.is_empty()).collect()
}

fn resolve_wordlist(language: &str) -> &'static Vec<&'static str> {
    let mut lang = normalize_language_code(language);
    if lang.is_empty() {
        return WORDLISTS
            .get("en")
            .expect("English wordlist must be present");
    }

    loop {
        if let Some(wordlist) = WORDLISTS.get(lang.as_str()) {
            return wordlist;
        }
        if let Some(pos) = lang.rfind('-') {
            lang.truncate(pos);
        } else {
            break;
        }
    }

    WORDLISTS
        .get("en")
        .expect("English wordlist must be present")
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

fn get_bit_msb(data: &[u8], bit_index: usize) -> bool {
    let byte_index = bit_index / 8;
    let bit_in_byte = 7 - (bit_index % 8);
    (data[byte_index] & (1 << bit_in_byte)) != 0
}
