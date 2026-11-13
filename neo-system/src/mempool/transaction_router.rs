//! Transaction router implementation.
//!
//! Distributed from `neo-core::ledger::transaction_router`. Provides the
//! `TransactionRouter` functionality matching C# Neo `TransactionRouter`.
//!
//! # Dependency adaptation
//!
//! The neo-core original used the `TransactionExt::verify_state_independent`
//! method. That extension trait lives in neo-core; the underlying logic is
//! exposed by `neo_ledger::validation::verify_transaction_state_independent`,
//! which this crate already consumes for its mempool validation path.

use neo_ledger::validation::verify_transaction_state_independent;
use neo_primitives::VerifyResult;
use neo_protocol::payloads::Transaction;
use neo_protocol::protocol_settings::ProtocolSettings;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Below this size, serial verification avoids parallel scheduling overhead.
const PARALLEL_PREVERIFY_THRESHOLD: usize = 4;

/// public record PreverifyCompleted(Transaction Transaction, bool Relay, VerifyResult Result);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreverifyCompleted {
    /// The transaction that was verified.
    pub transaction: Transaction,
    /// Whether the transaction should be relayed to the network after verification.
    pub relay: bool,
    /// The outcome of the state-independent verification.
    pub result: VerifyResult,
}

/// Transaction router for handling transaction pre-verification
pub struct TransactionRouter {
    settings: ProtocolSettings,
}

impl TransactionRouter {
    /// Constructor from protocol settings
    pub fn new(settings: ProtocolSettings) -> Self {
        Self { settings }
    }

    /// Runs state-independent transaction verification before blockchain validation.
    pub fn preverify(&self, transaction: Transaction, relay: bool) -> PreverifyCompleted {
        let result = verify_transaction_state_independent(&transaction, &self.settings);

        PreverifyCompleted {
            transaction,
            relay,
            result,
        }
    }

    /// Preverifies a bounded batch using parallel workers for CPU-bound script and
    /// signature checks. Indexed collection preserves input order and each result is
    /// computed from exactly the same inputs as [`Self::preverify`].
    ///
    /// This performs no state-dependent validation or mempool mutation. Callers must
    /// still admit the returned transactions sequentially so conflicts, sender limits,
    /// fee priority and verification-context updates retain their canonical semantics.
    pub fn preverify_batch(
        &self,
        transactions: Vec<(Transaction, bool)>,
    ) -> Vec<PreverifyCompleted> {
        if transactions.len() < PARALLEL_PREVERIFY_THRESHOLD {
            return transactions
                .into_iter()
                .map(|(transaction, relay)| self.preverify(transaction, relay))
                .collect();
        }

        transactions
            .into_par_iter()
            .map(|(transaction, relay)| self.preverify(transaction, relay))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm::OpCode;

    #[test]
    fn batch_preverify_matches_serial_results_and_order() {
        let router = TransactionRouter::new(ProtocolSettings::default());
        let transactions = (0..17)
            .map(|nonce| {
                let mut transaction = Transaction::new();
                transaction.set_nonce(nonce);
                transaction.set_script(vec![OpCode::PUSH1.byte()]);
                (transaction, nonce % 2 == 0)
            })
            .collect::<Vec<_>>();
        let expected = transactions
            .iter()
            .cloned()
            .map(|(transaction, relay)| router.preverify(transaction, relay))
            .collect::<Vec<_>>();

        let actual = router.preverify_batch(transactions);

        assert_eq!(actual.len(), expected.len());
        for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(actual.transaction.nonce(), index as u32);
            assert_eq!(actual.transaction.nonce(), expected.transaction.nonce());
            assert_eq!(actual.relay, expected.relay);
            assert_eq!(actual.result, expected.result);
        }
    }
}
