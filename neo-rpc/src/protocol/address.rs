//! Neo address and script-hash text decoding shared by RPC transports.

use neo_error::CoreResult;
use neo_primitives::UInt160;
use neo_wallets::wallet_helper::WalletAddress;

/// Parses either a hexadecimal script hash or an address for one Neo network.
pub(crate) fn parse_script_hash_or_address(text: &str, address_version: u8) -> CoreResult<UInt160> {
    let mut parsed = None;
    if UInt160::try_parse(text, &mut parsed) {
        if let Some(value) = parsed {
            return Ok(value);
        }
    }
    WalletAddress::to_script_hash(text, address_version)
}
