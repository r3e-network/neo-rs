//! Notary block-persist side effects.
//!
//! Keeps Notary-assisted fee accounting, deposit debits, designated-notary
//! reward calculation, and GAS minting out of the contract root.

use super::Notary;
use crate::{LedgerContract, Role, RoleManagement};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Contract};
use neo_payloads::TransactionAttribute;
use neo_primitives::{TransactionAttributeType, UInt160};
use num_bigint::BigInt;

impl Notary {
    /// C# `Notary.OnPersistAsync` (Notary.cs:61-90), run by the persist
    /// pipeline only while Notary is active (`ActiveIn = HF_Echidna`, gated
    /// by `is_active` in the dispatch loop).
    ///
    /// For every transaction in the persisting block carrying a
    /// `NotaryAssisted` attribute it (a) accumulates `nKeys + 1` into the
    /// fee count and (b) - when the transaction's sender is the Notary
    /// account itself - debits the payer's (`Signers[1]`) deposit by
    /// `SystemFee + NetworkFee`, removing the deposit at zero. After the
    /// loop it mints the per-notary reward `nFees *
    /// Policy.GetAttributeFeeV1(NotaryAssisted) / notaries.Length` (C#
    /// `CalculateNotaryReward`) to each designated P2PNotary node's
    /// signature-redeem-script hash. This is the reminting counterpart of
    /// the NotaryAssisted share `GasToken::on_persist` withholds from the
    /// primary-validator network-fee mint, so per-block GAS supply is
    /// conserved (matching C#, including the dropped integer-division
    /// remainder).
    pub(super) fn on_persist_native(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
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

        // C# `if (nFees == 0) return;` - no NotaryAssisted transactions.
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
        // the block (unreachable for a valid block - NotaryAssisted
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
}
