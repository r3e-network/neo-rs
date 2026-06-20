//! C# `TransactionAttribute.Verify` dispatch for the admission path.

use neo_native_contracts::ledger_contract::LedgerContract;
use neo_native_contracts::{NeoToken, Notary, OracleContract, RoleManagement};
use neo_payloads::{OracleResponse, Transaction, TransactionAttribute};
use neo_primitives::UInt160;
use neo_storage::DataCache;

use super::sender;

/// C# `TransactionAttribute.Verify` dispatch.
pub(super) fn verify_attribute(
    snapshot: &DataCache,
    tx: &Transaction,
    attribute: &TransactionAttribute,
    height: u32,
) -> bool {
    match attribute {
        // C# HighPriorityAttribute.Verify: a signer must be the committee
        // multisig address.
        TransactionAttribute::HighPriority => {
            let committee =
                neo_execution::NativeContract::committee_address(&NeoToken::new(), snapshot);
            match committee {
                Ok(Some(committee)) => tx.signers().iter().any(|s| s.account == committee),
                _ => false,
            }
        }
        // C# NotValidBefore.Verify: `CurrentIndex >= Height`.
        TransactionAttribute::NotValidBefore(attr) => height >= attr.height,
        // C# v3.10.0 Conflicts.Verify: reject if the transaction carries
        // duplicate Conflicts attributes referencing the same hash, then require
        // the conflicting hash not be an on-chain transaction.
        TransactionAttribute::Conflicts(attr) => {
            let mut seen = std::collections::HashSet::new();
            let has_duplicate = tx
                .attributes()
                .iter()
                .filter_map(|a| match a {
                    TransactionAttribute::Conflicts(c) => Some(c.hash),
                    _ => None,
                })
                .any(|hash| !seen.insert(hash));
            if has_duplicate {
                return false;
            }
            !LedgerContract::new()
                .contains_transaction(snapshot, &attr.hash)
                .unwrap_or(true)
        }
        // C# OracleResponse.Verify.
        TransactionAttribute::OracleResponse(attr) => {
            if tx
                .signers()
                .iter()
                .any(|s| s.scopes != neo_primitives::WitnessScope::NONE)
            {
                return false;
            }
            let fixed_script = OracleResponse::get_fixed_script();
            if tx.script() != fixed_script.as_slice() {
                return false;
            }
            let Ok(Some(request)) = OracleContract::new().get_request(snapshot, attr.id) else {
                return false;
            };
            if !oracle_response_gas_matches(tx, request.gas_for_response) {
                return false;
            }
            let Ok(oracles) = RoleManagement::new().get_designated_by_role_at(
                snapshot,
                neo_native_contracts::Role::Oracle,
                height + 1,
            ) else {
                return false;
            };
            let Some(oracle_account) = bft_address(&oracles) else {
                return false;
            };
            tx.signers().iter().any(|s| s.account == oracle_account)
        }
        // C# NotaryAssisted.Verify: the Notary hash must sign; when it is
        // the sender there must be exactly two signers (payer second).
        TransactionAttribute::NotaryAssisted(_) => {
            let notary = Notary::script_hash();
            if sender(tx) == Some(notary) {
                return tx.signers().len() == 2;
            }
            tx.signers().iter().any(|s| s.account == notary)
        }
    }
}

pub(super) fn oracle_response_gas_matches(tx: &Transaction, gas_for_response: i64) -> bool {
    tx.network_fee().wrapping_add(tx.system_fee()) == gas_for_response
}

/// C# `Contract.GetBFTAddress(pubkeys)` — `m = n - (n - 1) / 3` multisig
/// script hash; `None` for an empty designation.
fn bft_address(pubkeys: &[neo_crypto::ECPoint]) -> Option<UInt160> {
    if pubkeys.is_empty() {
        return None;
    }
    let m = pubkeys.len() - (pubkeys.len() - 1) / 3;
    let script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            m, pubkeys,
        )
        .ok()?;
    Some(UInt160::from_script(&script))
}
