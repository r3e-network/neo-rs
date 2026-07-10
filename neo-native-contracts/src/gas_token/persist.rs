//! GAS block-persist accounting.
//!
//! Keeps per-transaction fee burns, NotaryAssisted fee withholding, and
//! primary-validator network-fee minting out of the token root.

use super::GasToken;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_payloads::TransactionAttribute;
use neo_primitives::{TransactionAttributeType, UInt160};
use num_bigint::BigInt;

impl GasToken {
    /// C# `GasToken.OnPersistAsync` (GasToken.cs:39-58): for every transaction
    /// in the persisting block, burn the sender's `SystemFee + NetworkFee` and
    /// accumulate the network fee into the block total; a `NotaryAssisted`
    /// attribute redirects `(NKeys + 1) * AttributeFee(NotaryAssisted)` of that
    /// total to the designated notary nodes (minted by the Notary contract in
    /// its own `PostPersist`), so it is subtracted here. Finally mint the
    /// remaining total to the primary validator - the signature-contract
    /// address of `NEO.GetNextBlockValidators(...)[block.PrimaryIndex]` - with
    /// `callOnPayment: false`.
    ///
    /// The NotaryAssisted branch is not hardfork-gated in C# (the attribute is
    /// only valid in transactions once HF_Echidna verification admits it, so
    /// the gate is implicit), and `GetAttributeFeeV1` is the plain
    /// `Prefix_AttributeFee` storage read with the NotaryAssisted type allowed
    /// (PolicyContract.cs:278-301).
    pub(super) fn on_persist_native<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
    ) -> CoreResult<()> {
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
