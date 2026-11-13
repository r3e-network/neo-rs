//! Ledger hardware wallet support

mod ledger_signer;

pub use ledger_signer::LedgerSigner;

/// Neo BIP44 coin type
pub const NEO_COIN_TYPE: u32 = 888;

/// Build BIP44 derivation path for Neo
/// Format: m/44'/888'/account'/change/index
pub fn neo_derivation_path(account: u32, index: u32) -> String {
    format!("m/44'/{}'/{}'/{}/{}", NEO_COIN_TYPE, account, 0, index)
}

/// Parses and validates Neo's BIP44 derivation path.
///
/// Purpose, coin type, and account must be hardened; silently rewriting a
/// different path into a Neo path could make the user sign with another key.
pub fn parse_derivation_path(path: &str) -> Option<(u32, u32, u32, u32, u32)> {
    let path = path.strip_prefix("m/")?;
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() != 5
        || parts[0] != "44'"
        || parts[1] != "888'"
        || !parts[2].ends_with('\'')
        || parts[3].ends_with('\'')
        || parts[4].ends_with('\'')
    {
        return None;
    }

    let parse_component = |value: &str| -> Option<u32> {
        let value = value.parse::<u32>().ok()?;
        (value <= 0x7fff_ffff).then_some(value)
    };
    let account = parse_component(parts[2].strip_suffix('\'')?)?;
    let change = parse_component(parts[3])?;
    if change > 1 {
        return None;
    }
    let index = parse_component(parts[4])?;
    Some((44, NEO_COIN_TYPE, account, change, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neo_derivation_path_roundtrips_without_rewriting_components() {
        let path = neo_derivation_path(7, 9);
        assert_eq!(parse_derivation_path(&path), Some((44, 888, 7, 0, 9)));
        assert_eq!(
            parse_derivation_path("m/44'/888'/7'/1/9"),
            Some((44, 888, 7, 1, 9))
        );
    }

    #[test]
    fn invalid_or_non_neo_paths_are_rejected() {
        for path in [
            "44'/888'/0'/0/0",
            "m/44/888'/0'/0/0",
            "m/44'/60'/0'/0/0",
            "m/44'/888'/0/0/0",
            "m/44'/888'/0'/2/0",
            "m/44'/888'/0'/0/0'",
            "m/44'/888'/2147483648'/0/0",
        ] {
            assert_eq!(parse_derivation_path(path), None, "accepted {path}");
        }
    }
}
