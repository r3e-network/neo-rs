//! Transaction-related helpers for [`crate::service::BlockchainService`].
//!
//! The full transaction admission pipeline (storage conflict
//! detection, mempool conflict check, signer accounting) is the
//! focus of Stage C. Stage B keeps the surface minimal so the
//! `add_transaction` command on the handle is round-trip-able.

use neo_primitives::verify_result::VerifyResult;
use tracing::warn;

use crate::service::BlockchainService;

impl BlockchainService {
    /// Returns `true` if a transaction already exists on the chain.
    ///
    /// Stage B is a stub that returns `false`; the full ledger
    /// lookup lands in Stage C.
    pub(crate) fn transaction_exists_on_chain(&self, _tx: &neo_payloads::Transaction) -> bool {
        false
    }

    /// Returns `true` if a conflict record exists for the given
    /// transaction.
    ///
    /// Stage B is a stub that returns `false`; the full conflict
    /// detection lands in Stage C.
    pub(crate) fn conflict_exists_on_chain(
        &self,
        _tx: &neo_payloads::Transaction,
        _max_traceable_blocks: u32,
    ) -> bool {
        false
    }

    /// Validates a transaction. Stage B returns
    /// `VerifyResult::Succeed` after a trivial hash check; the full
    /// pipeline lands in Stage C.
    pub(crate) fn validate_transaction(
        &self,
        tx: &neo_payloads::Transaction,
    ) -> VerifyResult {
        match tx.try_hash() {
            Ok(_) => VerifyResult::Succeed,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction validation: hash computation failed"
                );
                VerifyResult::Invalid
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::BlockchainHandle;
    use crate::header_cache::HeaderCache;
    use crate::ledger_context::LedgerContext;
    use crate::service::MempoolLike;
    use crate::service_context::SystemContext;
    use neo_payloads::Transaction;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestContext;
    impl SystemContext for TestContext {
        fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
            Arc::new(neo_config::ProtocolSettings::default())
        }
        fn current_height(&self) -> u32 {
            0
        }
    }

    #[derive(Debug, Default)]
    struct TestMempool;
    impl MempoolLike for TestMempool {
        fn try_add(
            &self,
            _tx: &Transaction,
            _snapshot: &neo_data_cache::DataCache,
            _settings: &neo_config::ProtocolSettings,
        ) -> VerifyResult {
            VerifyResult::Succeed
        }
    }

    fn fixture() -> (BlockchainService, BlockchainHandle) {
        let system: Arc<dyn SystemContext> = Arc::new(TestContext);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::default());
        let mempool: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(TestMempool));
        BlockchainService::with_defaults(system, ledger, header_cache, mempool)
    }

    #[test]
    fn transaction_exists_on_chain_returns_false() {
        let (service, _handle) = fixture();
        let tx = Transaction::new();
        assert!(!service.transaction_exists_on_chain(&tx));
    }

    #[test]
    fn conflict_exists_on_chain_returns_false() {
        let (service, _handle) = fixture();
        let tx = Transaction::new();
        assert!(!service.conflict_exists_on_chain(&tx, 100));
    }

    #[test]
    fn validate_transaction_returns_succeed() {
        let (service, _handle) = fixture();
        let tx = Transaction::new();
        assert_eq!(service.validate_transaction(&tx), VerifyResult::Succeed);
    }
}
