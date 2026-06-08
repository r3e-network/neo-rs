//! [`TransactionRouter`] - the entry point for transactions received
//! from the network.
//!
//! Performs the cheap, state-independent verification
//! (signature shape, size limits, fee bounds, etc.) before the
//! transaction is admitted into the [`crate::MemoryPool`] for
//! state-dependent (witness) verification.

use neo_config::ProtocolSettings;
use neo_payloads::Transaction;
use neo_primitives::{Verifiable, VerifyResult};

/// Result of the state-independent pre-verification stage.
#[derive(Debug, Clone)]
pub struct PreverifyCompleted {
    /// The transaction that was pre-verified.
    pub transaction: Transaction,
    /// Whether the transaction was originally intended to be
    /// relayed (true) or merely accepted locally (false).
    pub relay: bool,
    /// The outcome of the state-independent verification.
    pub result: VerifyResult,
}

impl PreverifyCompleted {
    /// Returns whether the pre-verification succeeded.
    pub fn is_success(&self) -> bool {
        self.result.is_success()
    }
}

/// Router for state-independent transaction pre-verification.
#[derive(Debug, Clone)]
pub struct TransactionRouter {
    settings: ProtocolSettings,
}

impl TransactionRouter {
    /// Constructs a new `TransactionRouter` from the supplied
    /// protocol settings.
    pub fn new(settings: ProtocolSettings) -> Self {
        Self { settings }
    }

    /// Returns the protocol settings this router was constructed with.
    pub fn settings(&self) -> &ProtocolSettings {
        &self.settings
    }

    /// Runs state-independent transaction verification.
    pub fn preverify(&self, transaction: Transaction, relay: bool) -> PreverifyCompleted {
        let succeeded = Verifiable::verify(&transaction);
        let result = if succeeded { VerifyResult::Succeed } else { VerifyResult::Invalid };
        PreverifyCompleted {
            transaction,
            relay,
            result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_payloads::{Signer, Transaction, Witness};
    use neo_primitives::{UInt160, WitnessScope};
    use neo_vm_rs::OpCode;

    fn sample_tx() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(0);
        tx.set_network_fee(1);
        tx.set_script(vec![OpCode::RET.byte()]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn preverify_accepts_well_formed_transaction() {
        let router = TransactionRouter::new(ProtocolSettings::default());
        let result = router.preverify(sample_tx(), true);
        assert!(result.is_success());
        assert!(result.relay);
    }
}
