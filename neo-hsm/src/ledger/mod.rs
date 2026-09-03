//! Ledger hardware wallet support

#[cfg(feature = "ledger")]
mod ledger_signer;

#[cfg(feature = "ledger")]
pub use ledger_signer::LedgerSigner;

/// Neo BIP44 coin type
pub const NEO_COIN_TYPE: u32 = 888;

/// Build BIP44 derivation path for Neo
/// Format: m/44'/888'/account'/change/index
pub fn neo_derivation_path(account: u32, index: u32) -> String {
    format!("m/44'/{}'/{}'/{}/{}", NEO_COIN_TYPE, account, 0, index)
}

/// Parse a BIP44 derivation path into components
pub fn parse_derivation_path(path: &str) -> Option<(u32, u32, u32, u32, u32)> {
    let path = path.trim_start_matches("m/");
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() != 5 {
        return None;
    }

    let parse_component = |s: &str| -> Option<u32> {
        let s = s.trim_end_matches('\'');
        s.parse().ok()
    };

    Some((
        parse_component(parts[0])?, // purpose (44)
        parse_component(parts[1])?, // coin_type (888)
        parse_component(parts[2])?, // account
        parse_component(parts[3])?, // change
        parse_component(parts[4])?, // index
    ))
}
