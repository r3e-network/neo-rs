//! # neo-native-contracts::notary
//!
//! Native Notary contract state and request verification behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `invoke`: native method dispatch for deposit/withdraw/verify calls.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.
//! - `verify_dispatch_tests`: notary dispatch verification coverage.

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeMethod};
use neo_payloads::TransactionAttribute;
use neo_primitives::{TransactionAttributeType, UInt160};
use neo_storage::StorageItem;
use num_bigint::BigInt;

use crate::hashes::NOTARY_HASH;
use crate::{LedgerContract, Role, RoleManagement};

mod invoke;
mod metadata;
mod storage;

/// C# `Notary.DefaultDepositDeltaTill`: the default lock-height delta applied to a
/// first deposit whose `till` the depositor isn't allowed to set itself.
const DEFAULT_DEPOSIT_DELTA_TILL: u32 = 5760;

/// Storage prefix for the max-NotValidBefore-delta setting (C#
/// `Notary.Prefix_MaxNotValidBeforeDelta`).
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;
/// C# `Notary.DefaultMaxNotValidBeforeDelta`.
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: i64 = 140;
/// C# `Notary.Prefix_Deposit` — per-account deposit (`Struct[Amount, Till]`).
const PREFIX_DEPOSIT: u8 = 1;

native_contract_handle!(
    /// The Notary native contract.
    pub struct Notary {
        id: -10,
        contract_name: "Notary",
        hash: NOTARY_HASH,
    }
);

impl NativeContract for Notary {
    native_contract_identity!(Notary);

    // C# `Notary.Activations => [Hardfork.HF_Echidna, Hardfork.HF_Faun]`
    // (Notary.cs): the contract itself does not exist before HF_Echidna —
    // `ActiveIn` is the first activation. Without this override the contract
    // would be genesis-active in neo-rs, diverging native deployment, manifest
    // state, and call resolution below the Echidna height.
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfEchidna)
    }

    fn activations(&self) -> &'static [Hardfork] {
        &[Hardfork::HfEchidna, Hardfork::HfFaun]
    }

    /// C# `Notary.OnManifestCompose` (Notary.cs:92-102): NEP-30 joins NEP-27
    /// once HF_Faun is enabled at the height.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            crate::native_supported_standards(&[crate::NEP27_STANDARD, crate::NEP30_STANDARD])
        } else {
            crate::native_supported_standards(&[crate::NEP27_STANDARD])
        }
    }

    fn methods(&self) -> &[NativeMethod] {
        &metadata::NOTARY_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    /// C# `Notary.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (Notary.cs:52-59; ActiveIn is HF_Echidna, so this runs while persisting
    /// the Echidna activation block): seed `Prefix_MaxNotValidBeforeDelta` with
    /// `DefaultMaxNotValidBeforeDelta` (140).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        engine.snapshot_cache().add(
            Self::max_not_valid_before_delta_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MAX_NOT_VALID_BEFORE_DELTA,
            ))),
        );
        Ok(())
    }

    /// C# `Notary.OnPersistAsync` (Notary.cs:61-90), run by the persist
    /// pipeline only while Notary is active (`ActiveIn = HF_Echidna`, gated
    /// by `is_active` in the dispatch loop).
    ///
    /// For every transaction in the persisting block carrying a
    /// `NotaryAssisted` attribute it (a) accumulates `nKeys + 1` into the
    /// fee count and (b) — when the transaction's sender is the Notary
    /// account itself — debits the payer's (`Signers[1]`) deposit by
    /// `SystemFee + NetworkFee`, removing the deposit at zero. After the
    /// loop it mints the per-notary reward `nFees *
    /// Policy.GetAttributeFeeV1(NotaryAssisted) / notaries.Length` (C#
    /// `CalculateNotaryReward`) to each designated P2PNotary node's
    /// signature-redeem-script hash. This is the reminting counterpart of
    /// the NotaryAssisted share `GasToken::on_persist` withholds from the
    /// primary-validator network-fee mint, so per-block GAS supply is
    /// conserved (matching C#, including the dropped integer-division
    /// remainder).
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let notary_hash = Self::script_hash();

        // Pass 1: under the persisting-block borrow, accumulate the fee
        // count and the Notary-paid deposit debits.
        let (n_fees, debits) = {
            let block =
                crate::support::engine::require_persisting_block(engine, "Notary::on_persist")?;
            let mut n_fees: i64 = 0;
            let mut debits: Vec<(UInt160, i64)> = Vec::new();
            for tx in &block.transactions {
                // C# `tx.GetAttribute<NotaryAssisted>()` (AllowMultiple=false).
                let Some(nkeys) = tx.attributes().iter().find_map(|attr| match attr {
                    TransactionAttribute::NotaryAssisted(na) => Some(na.nkeys),
                    _ => None,
                }) else {
                    continue;
                };
                n_fees += i64::from(nkeys) + 1;
                // C# `if (tx.Sender == Hash)`: the Notary pays the fees, so
                // debit the payer (`Signers[1]`) deposit.
                if tx.sender() == Some(notary_hash) {
                    let payer = tx.signers().get(1).ok_or_else(|| {
                        CoreError::invalid_operation(
                            "Notary::on_persist: notary-paid transaction has fewer than two signers",
                        )
                    })?;
                    // C# `tx.SystemFee + tx.NetworkFee` (unchecked long).
                    let fees = tx.system_fee().wrapping_add(tx.network_fee());
                    debits.push((payer.account, fees));
                }
            }
            (n_fees, debits)
        };

        // C# `if (nFees == 0) return;` — no NotaryAssisted transactions.
        if n_fees == 0 {
            return Ok(());
        }

        // Apply the deposit debits staged above (C# `GetAndChange(
        // Prefix_Deposit, payer)` inside the transaction loop): subtract the
        // fees, removing the deposit when it reaches zero.
        {
            let snapshot = engine.snapshot_cache();
            for (payer, fees) in &debits {
                if let Some((amount, till)) = self.read_deposit(&snapshot, payer)? {
                    let new_amount = amount - BigInt::from(*fees);
                    if new_amount.sign() == num_bigint::Sign::NoSign {
                        self.delete_deposit(&snapshot, payer);
                    } else {
                        self.write_deposit(&snapshot, payer, &new_amount, till)?;
                    }
                }
            }
        }

        // C# `GetNotaryNodes`: the P2PNotary designation effective at
        // `Ledger.CurrentIndex + 1`.
        let notaries = {
            let snapshot = engine.snapshot_cache();
            let current = LedgerContract::new().current_index(&snapshot)?;
            RoleManagement::new().get_designated_by_role_at(
                &snapshot,
                Role::P2PNotary,
                current.wrapping_add(1),
            )?
        };
        // C# divides the reward by `notaries.Length`; an empty designation
        // with NotaryAssisted fees would be a DivideByZeroException faulting
        // the block (unreachable for a valid block — NotaryAssisted
        // verification requires designated notaries).
        if notaries.is_empty() {
            return Err(CoreError::invalid_operation(
                "Notary::on_persist: NotaryAssisted fees with no designated P2PNotary nodes",
            ));
        }

        // C# `CalculateNotaryReward`: `nFees * GetAttributeFeeV1(
        // NotaryAssisted) / notaries.Length`, minted to each notary with
        // `callOnPayment = false`.
        let per_key = crate::PolicyContract::new().attribute_fee(
            &engine.snapshot_cache(),
            TransactionAttributeType::NotaryAssisted.to_byte(),
            true,
        )?;
        let single_reward = BigInt::from(n_fees.wrapping_mul(per_key) / notaries.len() as i64);
        for notary in notaries {
            let address = UInt160::from_script(&Contract::create_signature_redeem_script(notary));
            crate::GasToken::new().gas_mint(engine, &address, &single_reward, false)?;
        }
        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }
}

#[cfg(test)]
#[path = "../tests/notary/mod.rs"]
mod tests;

/// End-to-end coverage of `verify` through the VM dispatch (the proven
/// witness-gated script-execution harness): the Notary native is seeded via a
/// ContractManagement record, a P2PNotary designation is written in the
/// RoleManagement storage layout, and `verify(signature)` is exercised through
/// `System.Contract.Call` against NotaryAssisted transaction containers.
#[cfg(test)]
#[path = "../tests/notary/verify_dispatch_tests.rs"]
mod verify_dispatch_tests;
