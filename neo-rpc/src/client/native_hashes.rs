//! Static native-contract script hashes used by RPC client helpers.
//!
//! Client code only needs stable contract hashes for script construction and
//! balance probes. Use the associated `script_hash()` methods directly instead
//! of constructing native contract handles just to call `hash()`.

use neo_native_contracts::{GasToken, NeoToken, PolicyContract};
use neo_primitives::UInt160;

#[must_use]
pub(super) fn neo_hash() -> UInt160 {
    NeoToken::script_hash()
}

#[must_use]
pub(super) fn gas_hash() -> UInt160 {
    GasToken::script_hash()
}

#[must_use]
pub(super) fn policy_hash() -> UInt160 {
    PolicyContract::script_hash()
}
