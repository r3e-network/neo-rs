//! GasToken genesis initialization.
//!
//! Seeds the initial GAS distribution exactly as C# does while keeping the
//! native contract root focused on identity, metadata, hooks, and dispatch.

use super::GasToken;
use neo_error::CoreResult;
use neo_execution::ApplicationEngine;
use num_bigint::BigInt;

impl GasToken {
    /// C# `GasToken.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (GasToken.cs:29-37; GAS is genesis-active, so this runs while persisting
    /// block 0): mint `ProtocolSettings.InitialGasDistribution` GAS to the BFT
    /// address of the standby validators, with `callOnPayment: false`.
    pub(super) fn initialize_native(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let standby_validators = engine.protocol_settings().standby_validators();
        let initial = BigInt::from(engine.protocol_settings().initial_gas_distribution);
        let account = crate::NeoToken::bft_address(&standby_validators)?;
        self.gas_mint(engine, &account, &initial, false)
    }
}
