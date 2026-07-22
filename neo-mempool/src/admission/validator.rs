//! State-independent validation entry point.

use neo_config::ProtocolSettings;
use neo_payloads::Transaction;
use neo_primitives::VerifyResult;

use super::{
    TransactionOrigin, TransactionValidationOutcome, ValidatedTransaction,
    verification::verify_state_independent,
};

/// Runs the pure transaction checks that do not depend on canonical or pool
/// state. This function is deliberately called before acquiring the pool write
/// lock.
pub(crate) fn validate_state_independent(
    transaction: Transaction,
    origin: TransactionOrigin,
    settings: &ProtocolSettings,
) -> TransactionValidationOutcome {
    let result = verify_state_independent(&transaction, settings);
    if result == VerifyResult::Succeed {
        TransactionValidationOutcome::Valid(ValidatedTransaction::new(transaction, origin))
    } else {
        TransactionValidationOutcome::Rejected {
            transaction,
            origin,
            result,
        }
    }
}
