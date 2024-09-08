
use std::sync::Arc;

pub mod ledger {
    use NeoRust::builder::Transaction;
    use crate::ledger::verify_result::VerifyResult;
    use crate::neo_system::NeoSystem;
    use super::*;

    pub struct TransactionRouter {
        system: Arc<NeoSystem>,
    }

    pub struct Preverify {
        transaction: Transaction,
        relay: bool,
    }

    pub struct PreverifyCompleted {
        transaction: Transaction,
        relay: bool,
        result: VerifyResult,
    }

    impl TransactionRouter {
        pub fn new(system: Arc<NeoSystem>) -> Self {
            Self { system }
        }

        pub fn on_receive(&self, message: Preverify) -> Option<PreverifyCompleted> {
            let verify_result = message.transaction.verify_state_independent(&self.system.settings);
            Some(PreverifyCompleted {
                transaction: message.transaction,
                relay: message.relay,
                result: verify_result,
            })
        }

        pub fn props(system: Arc<NeoSystem>) -> Arc<TransactionRouter> {
            Arc::new(TransactionRouter::new(system))
        }
    }
}
