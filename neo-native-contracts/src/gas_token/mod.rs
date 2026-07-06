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
//! - `storage`: GAS account and total-supply storage helpers.
//! - `transfers`: GAS transfer, mint, and burn helpers.
//! - `tests`: Module-local tests and regression coverage.

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_payloads::TransactionAttribute;
use neo_primitives::{TransactionAttributeType, UInt160};
use num_bigint::BigInt;

use crate::hashes::GAS_TOKEN_HASH;

mod invoke;
mod metadata;
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

    /// C# `GasToken.OnPersistAsync` (GasToken.cs:39-58): for every transaction
    /// in the persisting block, burn the sender's `SystemFee + NetworkFee` and
    /// accumulate the network fee into the block total; a `NotaryAssisted`
    /// attribute redirects `(NKeys + 1) * AttributeFee(NotaryAssisted)` of that
    /// total to the designated notary nodes (minted by the Notary contract in
    /// its own `PostPersist`), so it is subtracted here. Finally mint the
    /// remaining total to the primary validator — the signature-contract
    /// address of `NEO.GetNextBlockValidators(...)[block.PrimaryIndex]` — with
    /// `callOnPayment: false`.
    ///
    /// The NotaryAssisted branch is not hardfork-gated in C# (the attribute is
    /// only valid in transactions once HF_Echidna verification admits it, so
    /// the gate is implicit), and `GetAttributeFeeV1` is the plain
    /// `Prefix_AttributeFee` storage read with the NotaryAssisted type allowed
    /// (PolicyContract.cs:278-301).
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        // Collect the per-transaction data under the shared block borrow; the
        // burns below need `&mut engine`.
        let (primary_index, tx_data) = {
            let block =
                crate::support::engine::require_persisting_block(engine, "GasToken::on_persist")?;
            let tx_data: Vec<(Option<UInt160>, i64, i64, Option<u8>)> = block
                .transactions
                .iter()
                .map(|tx| {
                    // C# `tx.GetAttribute<NotaryAssisted>()`: the first (and, by
                    // AllowMultiple=false, only) NotaryAssisted attribute.
                    let nkeys = tx.attributes().iter().find_map(|attr| match attr {
                        TransactionAttribute::NotaryAssisted(na) => Some(na.nkeys),
                        _ => None,
                    });
                    (tx.sender(), tx.system_fee(), tx.network_fee(), nkeys)
                })
                .collect();
            (usize::from(block.primary_index()), tx_data)
        };

        let mut total_network_fee: i64 = 0;
        for (sender, system_fee, network_fee, notary_nkeys) in tx_data {
            // C# `tx.Sender` is `Signers[0].Account`; a signerless transaction
            // cannot appear in a valid block (C# would throw on the indexer).
            let sender = sender.ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist: transaction has no sender")
            })?;
            let fee = system_fee.checked_add(network_fee).ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist: fee overflow")
            })?;
            self.gas_burn(engine, &sender, &BigInt::from(fee))?;
            total_network_fee = total_network_fee.checked_add(network_fee).ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist: network fee overflow")
            })?;
            if let Some(nkeys) = notary_nkeys {
                // C# `(notaryAssisted.NKeys + 1) * Policy.GetAttributeFeeV1(
                // snapshot, (byte)notaryAssisted.Type)`.
                let per_key = crate::PolicyContract::new().attribute_fee(
                    &engine.snapshot_cache(),
                    TransactionAttributeType::NotaryAssisted.to_byte(),
                    true,
                )?;
                total_network_fee -= (i64::from(nkeys) + 1) * per_key;
            }
        }

        // C# `NEO.GetNextBlockValidators(snapshot, settings.ValidatorsCount)`,
        // indexed by the persisting block's PrimaryIndex; an index outside the
        // validator set faults the block (C# IndexOutOfRangeException).
        let validators_count =
            usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
        let snapshot = engine.snapshot_cache();
        let primary = crate::NeoToken::new().next_block_validator_account(
            &snapshot,
            validators_count,
            primary_index,
        )?;
        self.gas_mint(engine, &primary, &BigInt::from(total_network_fee), false)
    }
}

#[cfg(test)]
use transfers::GasTransferOutcome;

#[cfg(test)]
#[path = "../tests/gas_token/mod.rs"]
mod tests;
