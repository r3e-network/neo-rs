//! Admission gates that sit between a relayed transaction and the pool.
//!
//! A transaction is only worth pooling if the chain has not already settled it.
//! [`admit_transaction`] therefore runs the three ledger-facing gates before
//! handing the transaction to [`MemoryPool::try_add`], which owns the
//! pool-internal policy (structure, fees, pool conflicts, capacity):
//!
//! 1. **membership** — the pool already holds the hash;
//! 2. **persistence** — the chain already committed the hash;
//! 3. **conflicts** — a persisted transaction signed by one of the same signers
//!    is still inside the traceable window and claims the hash.
//!
//! Both chain gates are *fail-closed*: when the chain cannot be read, the
//! transaction is reported [`VerifyResult::Invalid`] rather than admitted on
//! unverified state.
//!
//! The chain state the gates read is reached through [`AdmissionChainPort`], so
//! this module names no store, actor or system type: the node host implements
//! the port over whatever snapshot it holds and gets a verdict back.

use crate::memory_pool::MemoryPool;
use neo_primitives::{UInt160, UInt256, VerifyResult};
use neo_protocol::payloads::Transaction;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_storage::persistence::PersistenceDataCache;
use std::fmt;

/// Result alias for an [`AdmissionChainPort`] query.
pub type ChainQueryResult<T> = Result<T, ChainQueryError>;

/// Which chain question failed, so an operator can tell a store fault from a
/// policy-read fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainQuery {
    /// "Is this transaction already committed?"
    TransactionExistence,
    /// "Does a committed transaction conflict with this one?"
    ConflictExistence,
}

impl fmt::Display for ChainQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChainQuery::TransactionExistence => "transaction existence",
            ChainQuery::ConflictExistence => "conflict existence",
        })
    }
}

/// A chain query the node host could not answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainQueryError {
    query: ChainQuery,
    message: String,
}

impl ChainQueryError {
    /// Creates a query failure for `query` from the port's own message.
    #[must_use]
    pub fn new(query: ChainQuery, message: impl Into<String>) -> Self {
        Self {
            query,
            message: message.into(),
        }
    }

    /// Returns the chain question that failed.
    #[must_use]
    pub fn query(&self) -> ChainQuery {
        self.query
    }

    /// Returns the port-owned failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ChainQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} query failed: {}", self.query, self.message)
    }
}

impl std::error::Error for ChainQueryError {}

/// The persisted chain state mempool admission reads.
///
/// Implemented by the party that holds the store snapshot (the node's system
/// context); [`admit_transaction`] never sees a concrete store. Every method is
/// a narrow question — the port decides nothing.
pub trait AdmissionChainPort {
    /// The snapshot state-dependent verification and insertion must read.
    ///
    /// It is the same snapshot the queries below answer from, so a transaction
    /// cannot be admitted against one view of the chain and verified against
    /// another.
    fn snapshot(&self) -> &PersistenceDataCache;

    /// Height of the persisted chain tip, used for diagnostics.
    fn chain_height(&self) -> u32;

    /// Returns whether `hash` is already committed to the persisted chain.
    fn contains_transaction(&self, hash: &UInt256) -> ChainQueryResult<bool>;

    /// Returns the traceable window used for persisted conflict lookups.
    fn max_traceable_blocks(&self) -> u32;

    /// Returns whether `hash` conflicts with a persisted transaction signed by
    /// one of `signers` inside `max_traceable_blocks`.
    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> ChainQueryResult<bool>;
}

/// Admits `transaction` into `pool`, returning the verdict.
///
/// [`VerifyResult::Succeed`] means the transaction is in the verified set. Any
/// other verdict means it is not, and the verdict names the gate that refused it
/// or the pool check [`MemoryPool::try_add`] reported.
///
/// # Fail-closed chain reads
///
/// A transaction whose hash cannot be computed is [`VerifyResult::Invalid`], and
/// so is one whose chain gates cannot be answered: a chain the node cannot read
/// is not evidence that the chain accepted the transaction.
#[must_use]
pub fn admit_transaction(
    pool: &mut MemoryPool,
    transaction: &Transaction,
    chain: &dyn AdmissionChainPort,
    settings: &ProtocolSettings,
) -> VerifyResult {
    let hash = match transaction.try_hash() {
        Ok(hash) => hash,
        Err(error) => {
            tracing::warn!(
                target: "neo::mempool",
                error = %error,
                "transaction hash computation failed before mempool admission"
            );
            return VerifyResult::Invalid;
        }
    };

    if pool.contains_key(&hash) {
        return VerifyResult::AlreadyInPool;
    }

    match chain.contains_transaction(&hash) {
        Ok(true) => {
            tracing::debug!(
                target: "neo::mempool",
                %hash,
                height = chain.chain_height(),
                "refusing to pool a transaction the chain already committed"
            );
            return VerifyResult::AlreadyExists;
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                target: "neo::mempool",
                %error,
                %hash,
                "chain transaction-existence query failed; refusing admission (fail-closed)"
            );
            return VerifyResult::Invalid;
        }
    }

    let signers: Vec<UInt160> = transaction
        .signers()
        .iter()
        .map(|signer| signer.account)
        .collect();
    if !signers.is_empty() {
        let max_traceable_blocks = chain.max_traceable_blocks();
        match chain.contains_conflict_hash(&hash, &signers, max_traceable_blocks) {
            Ok(true) => {
                tracing::debug!(
                    target: "neo::mempool",
                    %hash,
                    height = chain.chain_height(),
                    max_traceable_blocks,
                    "refusing to pool a transaction that conflicts with persisted state"
                );
                return VerifyResult::HasConflicts;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    target: "neo::mempool",
                    %error,
                    %hash,
                    "chain conflict query failed; refusing admission (fail-closed)"
                );
                return VerifyResult::Invalid;
            }
        }
    }

    pool.try_add(transaction.clone(), chain.snapshot(), settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_native_contracts::AccountState;
    use neo_native_contracts::fungible_token::PREFIX_ACCOUNT;
    use neo_native_contracts::gas_token::GasToken;
    use neo_native_contracts::native_contract::NativeContract;
    use neo_primitives::WitnessScope;
    use neo_protocol::get_sign_data_vec;
    use neo_protocol::payloads::{Signer, Witness};
    use neo_storage::{StorageItem, StorageKey};
    use neo_vm::{BinarySerializer, ExecutionEngineLimits, OpCode};
    use neo_wallets::KeyPair;
    use num_bigint::BigInt;
    use std::cell::RefCell;
    use std::collections::HashSet;

    /// Chain port stub whose answers the test dictates.
    struct FakeChain {
        snapshot: PersistenceDataCache,
        height: u32,
        max_traceable_blocks: u32,
        committed: HashSet<UInt256>,
        conflicting: HashSet<UInt256>,
        fail: Option<ChainQuery>,
        seen_conflict_signers: RefCell<Vec<UInt160>>,
    }

    impl FakeChain {
        fn new(snapshot: PersistenceDataCache) -> Self {
            Self {
                snapshot,
                height: 1,
                max_traceable_blocks: 2_100_000,
                committed: HashSet::new(),
                conflicting: HashSet::new(),
                fail: None,
                seen_conflict_signers: RefCell::new(Vec::new()),
            }
        }
    }

    impl AdmissionChainPort for FakeChain {
        fn snapshot(&self) -> &PersistenceDataCache {
            &self.snapshot
        }

        fn chain_height(&self) -> u32 {
            self.height
        }

        fn contains_transaction(&self, hash: &UInt256) -> ChainQueryResult<bool> {
            if self.fail == Some(ChainQuery::TransactionExistence) {
                return Err(ChainQueryError::new(
                    ChainQuery::TransactionExistence,
                    "injected existence failure",
                ));
            }
            Ok(self.committed.contains(hash))
        }

        fn max_traceable_blocks(&self) -> u32 {
            self.max_traceable_blocks
        }

        fn contains_conflict_hash(
            &self,
            hash: &UInt256,
            signers: &[UInt160],
            _max_traceable_blocks: u32,
        ) -> ChainQueryResult<bool> {
            if self.fail == Some(ChainQuery::ConflictExistence) {
                return Err(ChainQueryError::new(
                    ChainQuery::ConflictExistence,
                    "injected conflict failure",
                ));
            }
            *self.seen_conflict_signers.borrow_mut() = signers.to_vec();
            Ok(self.conflicting.contains(hash))
        }
    }

    fn funded_snapshot(account: UInt160, amount: i64) -> PersistenceDataCache {
        let snapshot = PersistenceDataCache::new(false);
        let key = StorageKey::create_with_uint160(GasToken::new().id(), PREFIX_ACCOUNT, &account);
        let state = AccountState::with_balance(BigInt::from(amount));
        let bytes = BinarySerializer::serialize_stack_value(
            &state.to_stack_value(),
            &ExecutionEngineLimits::default(),
        )
        .expect("serialize account state");
        snapshot.update(key, StorageItem::from_bytes(bytes));
        snapshot
    }

    struct Fixture {
        settings: ProtocolSettings,
        pool: MemoryPool,
        private_key: [u8; 32],
        account: UInt160,
        chain: FakeChain,
    }

    fn fixture() -> Fixture {
        let settings = ProtocolSettings::default();
        let pool = MemoryPool::new(&settings);
        let private_key = [7u8; 32];
        let keypair = KeyPair::from_private_key(&private_key).expect("keypair");
        let account = keypair.get_script_hash();
        let chain = FakeChain::new(funded_snapshot(account, 50_0000_0000));
        Fixture {
            settings,
            pool,
            private_key,
            account,
            chain,
        }
    }

    fn signed_transaction(settings: &ProtocolSettings, private_key: [u8; 32]) -> Transaction {
        let keypair = KeyPair::from_private_key(&private_key).expect("keypair");
        let mut tx = Transaction::new();
        tx.set_network_fee(1_0000_0000);
        tx.set_system_fee(30);
        tx.set_valid_until_block(100);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);
        tx.set_attributes(Vec::new());

        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("sign");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        tx.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            keypair.get_verification_script(),
        )]);
        tx
    }

    #[test]
    fn admit_inserts_an_uncommitted_unconflicting_transaction() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            account,
            chain,
        } = fixture();
        let tx = signed_transaction(&settings, private_key);

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::Succeed
        );
        assert!(pool.contains_key(&tx.hash()));
        // The conflict gate asked with the transaction's signers.
        assert_eq!(*chain.seen_conflict_signers.borrow(), vec![account]);
    }

    #[test]
    fn admit_rejects_a_transaction_the_pool_already_holds() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            chain,
            ..
        } = fixture();
        let tx = signed_transaction(&settings, private_key);
        pool.try_add(tx.clone(), chain.snapshot(), &settings);

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::AlreadyInPool
        );
    }

    #[test]
    fn admit_rejects_a_transaction_the_chain_already_committed() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            mut chain,
            ..
        } = fixture();
        let tx = signed_transaction(&settings, private_key);
        chain.committed.insert(tx.hash());

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::AlreadyExists
        );
        assert!(!pool.contains_key(&tx.hash()));
    }

    #[test]
    fn admit_rejects_a_transaction_conflicting_with_persisted_state() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            mut chain,
            ..
        } = fixture();
        let tx = signed_transaction(&settings, private_key);
        chain.conflicting.insert(tx.hash());

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::HasConflicts
        );
        assert!(!pool.contains_key(&tx.hash()));
    }

    #[test]
    fn admit_fails_closed_when_the_existence_query_fails() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            mut chain,
            ..
        } = fixture();
        let tx = signed_transaction(&settings, private_key);
        chain.fail = Some(ChainQuery::TransactionExistence);

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::Invalid
        );
        assert!(!pool.contains_key(&tx.hash()));
    }

    #[test]
    fn admit_fails_closed_when_the_conflict_query_fails() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            mut chain,
            ..
        } = fixture();
        let tx = signed_transaction(&settings, private_key);
        chain.fail = Some(ChainQuery::ConflictExistence);

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::Invalid
        );
        assert!(!pool.contains_key(&tx.hash()));
    }

    #[test]
    fn admit_rejects_an_unhashable_transaction_before_any_chain_query() {
        let Fixture {
            settings,
            mut pool,
            private_key,
            chain,
            ..
        } = fixture();
        let mut tx = signed_transaction(&settings, private_key);
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert_eq!(
            admit_transaction(&mut pool, &tx, &chain, &settings),
            VerifyResult::Invalid
        );
        assert_eq!(*chain.seen_conflict_signers.borrow(), Vec::<UInt160>::new());
    }

    #[test]
    fn chain_query_error_display_names_the_failed_question() {
        let error = ChainQueryError::new(ChainQuery::ConflictExistence, "store offline");
        assert_eq!(
            error.to_string(),
            "conflict existence query failed: store offline"
        );
        assert_eq!(error.query(), ChainQuery::ConflictExistence);
        assert_eq!(error.message(), "store offline");
    }
}
