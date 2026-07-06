//! # neo-native-contracts::gas_token
//!
//! Native GAS token state, accounting, and transfer behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `invoke`: NEP-17 native method dispatch.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `persist`: block-persist fee burn and primary reward accounting.
//! - `storage`: GAS account and total-supply storage helpers.
//! - `transfers`: GAS transfer, mint, and burn helpers.
//! - `tests`: Module-local tests and regression coverage.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use num_bigint::BigInt;

use crate::hashes::GAS_TOKEN_HASH;

mod invoke;
mod metadata;
mod persist;
mod storage;
mod transfers;

native_contract_handle!(
    /// The GasToken native contract.
    pub struct GasToken {
        id: -6,
        contract_name: "GasToken",
        hash: GAS_TOKEN_HASH,
    }
);

impl GasToken {
    /// NEP-17 symbol (C# `GasToken.Symbol => "GAS"`).
    pub const SYMBOL: &'static str = "GAS";
    /// NEP-17 decimals (C# `GasToken.Decimals => 8`).
    pub const DECIMALS: u8 = 8;
}

impl NativeContract for GasToken {
    native_contract_identity!(GasToken);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::GAS_TOKEN_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    /// C# `FungibleToken.OnManifestCompose` (FungibleToken.cs:68-71): every
    /// fungible token declares NEP-17 unconditionally.
    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        crate::native_supported_standards(&[crate::NEP17_STANDARD])
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::GAS_TOKEN_EVENTS
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }

    /// C# `GasToken.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (GasToken.cs:29-37; GAS is genesis-active, so this runs while persisting
    /// block 0): mint `ProtocolSettings.InitialGasDistribution` GAS to the BFT
    /// address of the standby validators, with `callOnPayment: false`.
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let standby_validators = engine.protocol_settings().standby_validators();
        let initial = BigInt::from(engine.protocol_settings().initial_gas_distribution);
        let account = crate::NeoToken::bft_address(&standby_validators)?;
        self.gas_mint(engine, &account, &initial, false)
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.on_persist_native(engine)
    }
}

#[cfg(test)]
use transfers::GasTransferOutcome;

#[cfg(test)]
#[path = "../tests/gas_token/mod.rs"]
mod tests;
