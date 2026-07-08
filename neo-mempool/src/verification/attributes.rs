//! C# `TransactionAttribute.Verify` dispatch for the admission path.

use neo_payloads::{OracleResponse, Transaction, TransactionAttribute};
use neo_storage::DataCache;
use neo_vm::script_builder::RedeemScript;

use super::super::native_provider::AdmissionNativeProvider;
use super::AdmissionLedgerProvider;
use super::sender;

/// C# `TransactionAttribute.Verify` dispatch.
pub(super) fn verify_attribute<L, N>(
    ledger: &L,
    native: &N,
    snapshot: &DataCache,
    tx: &Transaction,
    attribute: &TransactionAttribute,
    height: u32,
) -> bool
where
    L: AdmissionLedgerProvider + ?Sized,
    N: AdmissionNativeProvider + ?Sized,
{
    match attribute {
        // C# HighPriorityAttribute.Verify: a signer must be the committee
        // multisig address.
        TransactionAttribute::HighPriority => match native.committee_address(snapshot) {
            Ok(Some(committee)) => tx.signers().iter().any(|s| s.account == committee),
            _ => false,
        },
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
            !ledger
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
            let Ok(Some(request)) = native.oracle_request(snapshot, attr.id) else {
                return false;
            };
            if !oracle_response_gas_matches(tx, request.gas_for_response) {
                return false;
            }
            let Ok(oracles) = native.designated_oracles(snapshot, height + 1) else {
                return false;
            };
            let Some(oracle_account) = RedeemScript::bft_address(&oracles) else {
                return false;
            };
            tx.signers().iter().any(|s| s.account == oracle_account)
        }
        // C# NotaryAssisted.Verify: the Notary hash must sign; when it is
        // the sender there must be exactly two signers (payer second).
        TransactionAttribute::NotaryAssisted(_) => {
            let notary = native.notary_hash();
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
