use super::{DependencyCaptureLimits, TransactionDependencyCapture};
use neo_primitives::UInt256;
use neo_storage::{CacheRead, DataCache, StorageKey, Trackable};
use std::sync::Arc;

/// Exact canonical block prefix against which speculative work was started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockPrefixIdentity {
    block_hash: UInt256,
    block_index: u32,
    applied_transactions: usize,
}

impl BlockPrefixIdentity {
    /// Creates an identity for the prefix after `applied_transactions` canonical transactions.
    #[must_use]
    pub const fn new(block_hash: UInt256, block_index: u32, applied_transactions: usize) -> Self {
        Self {
            block_hash,
            block_index,
            applied_transactions,
        }
    }

    /// Hash of the immutable block containing the prefix.
    #[must_use]
    pub const fn block_hash(&self) -> UInt256 {
        self.block_hash
    }

    /// Index of the immutable block containing the prefix.
    #[must_use]
    pub const fn block_index(&self) -> u32 {
        self.block_index
    }

    /// Number of transactions already applied to the captured prefix.
    #[must_use]
    pub const fn applied_transactions(&self) -> usize {
        self.applied_transactions
    }
}

/// Failure to construct a transaction overlay for a pinned prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OptimisticOverlayError {
    /// A transaction that is already part of the prefix cannot be speculated from it.
    #[error(
        "transaction index {transaction_index} precedes pinned prefix length {applied_transactions}"
    )]
    TransactionPrecedesPrefix {
        /// Requested zero-based transaction position.
        transaction_index: usize,
        /// Transactions already represented by the prefix.
        applied_transactions: usize,
    },
}

/// Private copy of one canonical block prefix shared by isolated transaction overlays.
pub struct PinnedBlockPrefix<B: CacheRead> {
    identity: BlockPrefixIdentity,
    snapshot: Arc<DataCache<B>>,
}

impl<B: CacheRead> PinnedBlockPrefix<B> {
    /// Copies the prefix cache state so later canonical mutations cannot change this view.
    ///
    /// The cache backing supplied by the caller must itself be a point-in-time
    /// store snapshot, as it is in the block-persistence pipeline.
    #[must_use]
    pub(crate) fn capture(identity: BlockPrefixIdentity, prefix: &DataCache<B>) -> Self {
        Self {
            identity,
            snapshot: Arc::new(prefix.fork_isolated()),
        }
    }

    /// Returns the exact block-prefix identity.
    #[must_use]
    pub const fn identity(&self) -> BlockPrefixIdentity {
        self.identity
    }

    /// Creates a writable, non-publishing overlay for one transaction position.
    pub(crate) fn transaction_overlay(
        &self,
        transaction_index: usize,
    ) -> Result<IsolatedTransactionOverlay<B>, OptimisticOverlayError> {
        if transaction_index < self.identity.applied_transactions {
            return Err(OptimisticOverlayError::TransactionPrecedesPrefix {
                transaction_index,
                applied_transactions: self.identity.applied_transactions,
            });
        }
        Ok(IsolatedTransactionOverlay {
            prefix: self.identity,
            transaction_index,
            snapshot: Arc::new(self.snapshot.clone_detached_cache()),
            dependency_capture: None,
        })
    }

    /// Creates an isolated overlay with bounded point-read capture installed.
    ///
    /// The observer is bound before the cache is returned or passed to an
    /// `ApplicationEngine`, so constructor-time policy and native reads are
    /// included. Range reads mark the capture unsupported until range
    /// generation validation is implemented. Ordinary [`Self::transaction_overlay`]
    /// does not install a dependency observer.
    pub(crate) fn transaction_overlay_with_dependency_capture(
        &self,
        transaction_index: usize,
        limits: DependencyCaptureLimits,
    ) -> Result<(IsolatedTransactionOverlay<B>, TransactionDependencyCapture), OptimisticOverlayError>
    {
        if transaction_index < self.identity.applied_transactions {
            return Err(OptimisticOverlayError::TransactionPrecedesPrefix {
                transaction_index,
                applied_transactions: self.identity.applied_transactions,
            });
        }
        let capture = TransactionDependencyCapture::new(limits);
        let snapshot = self
            .snapshot
            .clone_detached_cache()
            .with_read_observer(capture.observer());
        Ok((
            IsolatedTransactionOverlay {
                prefix: self.identity,
                transaction_index,
                snapshot: Arc::new(snapshot),
                dependency_capture: Some(capture.clone()),
            },
            capture,
        ))
    }
}

/// Transaction-owned storage overlay whose root cannot publish into its pinned prefix.
pub struct IsolatedTransactionOverlay<B: CacheRead> {
    prefix: BlockPrefixIdentity,
    transaction_index: usize,
    snapshot: Arc<DataCache<B>>,
    dependency_capture: Option<TransactionDependencyCapture>,
}

impl<B: CacheRead> IsolatedTransactionOverlay<B> {
    /// Prefix against which this transaction executes.
    #[must_use]
    pub const fn prefix(&self) -> BlockPrefixIdentity {
        self.prefix
    }

    /// Canonical zero-based transaction position.
    #[must_use]
    pub const fn transaction_index(&self) -> usize {
        self.transaction_index
    }

    /// Returns the exact existing `DataCache` handle used by `ApplicationEngine`.
    #[must_use]
    pub(crate) fn snapshot_cache(&self) -> Arc<DataCache<B>> {
        Arc::clone(&self.snapshot)
    }

    pub(super) fn owns_dependency_capture(&self, capture: &TransactionDependencyCapture) -> bool {
        self.dependency_capture
            .as_ref()
            .is_some_and(|bound| bound.same_capture(capture))
    }

    /// Number of storage effects currently isolated in this transaction root.
    #[must_use]
    pub fn pending_storage_effects(&self) -> usize {
        self.snapshot.pending_change_count()
    }

    /// Visits isolated effects through the existing storage key/item representation.
    pub fn visit_storage_effects(&self, visitor: impl FnMut(&StorageKey, &Trackable)) {
        self.snapshot.visit_tracked_items(visitor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoDiagnostic;
    use crate::application_engine::{ApplicationEngine, TEST_MODE_GAS};
    use crate::native_contract_provider::{NativeContractProvider, NoNativeContract};
    use crate::optimistic_execution::DependencyCaptureError;
    use neo_config::ProtocolSettings;
    use neo_error::{CoreError, CoreResult};
    use neo_payloads::Block;
    use neo_primitives::TriggerType;
    use neo_storage::{DataCacheError, SeekDirection, StorageItem, TrackState};

    fn identity(applied_transactions: usize) -> BlockPrefixIdentity {
        BlockPrefixIdentity::new(UInt256::default(), 42, applied_transactions)
    }

    struct DependencyPolicyProvider;

    impl DependencyPolicyProvider {
        const EXEC_FEE_KEY: &'static [u8] = b"exec-fee";
        const STORAGE_PRICE_KEY: &'static [u8] = b"storage-price";
        const STORAGE_ID: i32 = 91;

        fn key(suffix: &[u8]) -> StorageKey {
            StorageKey::new(Self::STORAGE_ID, suffix.to_vec())
        }

        fn read_u32<B: CacheRead>(snapshot: &DataCache<B>, suffix: &[u8]) -> CoreResult<u32> {
            let bytes = snapshot
                .get(&Self::key(suffix))
                .ok_or_else(|| CoreError::invalid_operation("missing test policy value"))?
                .to_value();
            let bytes: [u8; 4] = bytes
                .try_into()
                .map_err(|_| CoreError::invalid_operation("invalid test policy value"))?;
            Ok(u32::from_le_bytes(bytes))
        }
    }

    impl NativeContractProvider for DependencyPolicyProvider {
        type Contract = NoNativeContract;

        fn exec_fee_factor_raw<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
            Self::read_u32(snapshot, Self::EXEC_FEE_KEY)
        }

        fn storage_price<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
            Self::read_u32(snapshot, Self::STORAGE_PRICE_KEY)
        }
    }

    #[test]
    fn captured_prefix_and_sibling_transaction_overlays_remain_isolated() {
        let key = StorageKey::new(7, vec![1]);
        let canonical = DataCache::new(false);
        canonical.add(key.clone(), StorageItem::from_bytes(b"prefix".to_vec()));
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);

        canonical.update(key.clone(), StorageItem::from_bytes(b"later".to_vec()));
        let first = prefix.transaction_overlay(0).expect("first overlay");
        let second = prefix.transaction_overlay(1).expect("second overlay");
        assert_eq!(
            first.snapshot_cache().get(&key).map(|item| item.to_value()),
            Some(b"prefix".to_vec())
        );
        first.snapshot_cache().update(
            key.clone(),
            StorageItem::from_bytes(b"speculative".to_vec()),
        );
        assert_eq!(
            second
                .snapshot_cache()
                .get(&key)
                .map(|item| item.to_value()),
            Some(b"prefix".to_vec())
        );
        assert_eq!(
            canonical.get(&key).map(|item| item.to_value()),
            Some(b"later".to_vec())
        );
    }

    #[test]
    fn detached_root_rejects_publication_and_retains_existing_effects() {
        let key = StorageKey::new(7, vec![1]);
        let canonical = DataCache::new(false);
        canonical.add(key.clone(), StorageItem::from_bytes(b"prefix".to_vec()));
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let overlay = prefix.transaction_overlay(0).expect("overlay");
        overlay.snapshot_cache().update(
            key.clone(),
            StorageItem::from_bytes(b"speculative".to_vec()),
        );

        assert_eq!(
            overlay.snapshot_cache().try_commit(),
            Err(DataCacheError::DetachedCommit)
        );
        assert_eq!(overlay.pending_storage_effects(), 1);
        let mut effects = Vec::new();
        overlay.visit_storage_effects(|effect_key, trackable| {
            effects.push((
                effect_key.clone(),
                trackable.state,
                trackable.item.to_value(),
            ));
        });
        assert_eq!(
            effects,
            vec![(key.clone(), TrackState::Changed, b"speculative".to_vec())]
        );
        assert_eq!(
            canonical.get(&key).map(|item| item.to_value()),
            Some(b"prefix".to_vec())
        );
    }

    #[test]
    fn nested_child_commit_merges_only_into_the_transaction_overlay() {
        let key = StorageKey::new(7, vec![1]);
        let canonical = DataCache::new(false);
        canonical.add(key.clone(), StorageItem::from_bytes(b"prefix".to_vec()));
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let overlay = prefix.transaction_overlay(0).expect("overlay");
        let child = overlay.snapshot_cache().clone_cache();
        child.update(key.clone(), StorageItem::from_bytes(b"nested".to_vec()));
        child.commit();

        assert_eq!(
            overlay
                .snapshot_cache()
                .get(&key)
                .map(|item| item.to_value()),
            Some(b"nested".to_vec())
        );
        assert_eq!(overlay.pending_storage_effects(), 1);
        assert_eq!(
            prefix
                .transaction_overlay(1)
                .expect("sibling")
                .snapshot_cache()
                .get(&key)
                .map(|item| item.to_value()),
            Some(b"prefix".to_vec())
        );
    }

    #[test]
    fn transactions_already_in_the_prefix_are_rejected() {
        let canonical = DataCache::new(false);
        let prefix = PinnedBlockPrefix::capture(identity(3), &canonical);
        assert_eq!(
            prefix.transaction_overlay(2).err(),
            Some(OptimisticOverlayError::TransactionPrecedesPrefix {
                transaction_index: 2,
                applied_transactions: 3,
            })
        );
        assert!(prefix.transaction_overlay(3).is_ok());
    }

    #[test]
    fn dependency_capture_records_present_and_absent_prefix_reads_once() {
        let present = StorageKey::new(7, b"present".to_vec());
        let absent = StorageKey::new(7, b"absent".to_vec());
        let canonical = DataCache::new(false);
        canonical.add(present.clone(), StorageItem::from_bytes(b"prefix".to_vec()));
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let (overlay, capture) = prefix
            .transaction_overlay_with_dependency_capture(0, DependencyCaptureLimits::default())
            .expect("tracked overlay");
        let cache = overlay.snapshot_cache();

        assert_eq!(
            cache.get(&present).map(|item| item.to_value()),
            Some(b"prefix".to_vec())
        );
        assert_eq!(cache.get(&absent), None);
        cache.add(
            absent.clone(),
            StorageItem::from_bytes(b"overlay-created".to_vec()),
        );
        assert_eq!(
            cache.get(&absent).map(|item| item.to_value()),
            Some(b"overlay-created".to_vec())
        );
        assert_eq!(
            cache.get(&present).map(|item| item.to_value()),
            Some(b"prefix".to_vec())
        );

        let dependencies = capture.snapshot().expect("supported point reads");
        assert_eq!(dependencies.point_reads().len(), 2);
        assert_eq!(dependencies.point_reads()[0].key(), &absent);
        assert_eq!(dependencies.point_reads()[0].value(), None);
        assert_eq!(dependencies.point_reads()[1].key(), &present);
        assert_eq!(
            dependencies.point_reads()[1]
                .value()
                .map(StorageItem::to_value),
            Some(b"prefix".to_vec())
        );
    }

    #[test]
    fn blind_delete_captures_the_implicit_pinned_read() {
        let key = StorageKey::new(7, b"deleted".to_vec());
        let canonical = DataCache::new(false);
        canonical.add(key.clone(), StorageItem::from_bytes(b"prefix".to_vec()));
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let (overlay, capture) = prefix
            .transaction_overlay_with_dependency_capture(0, DependencyCaptureLimits::default())
            .expect("tracked overlay");

        overlay.snapshot_cache().delete(&key);

        let dependencies = capture.snapshot().expect("point dependency");
        assert_eq!(dependencies.point_reads().len(), 1);
        assert_eq!(dependencies.point_reads()[0].key(), &key);
        assert_eq!(
            dependencies.point_reads()[0]
                .value()
                .map(StorageItem::to_value),
            Some(b"prefix".to_vec())
        );
    }

    #[test]
    fn overlay_origin_reads_do_not_become_prefix_dependencies() {
        let existing = StorageKey::new(7, b"existing".to_vec());
        let created = StorageKey::new(7, b"created".to_vec());
        let canonical = DataCache::new(false);
        canonical.add(
            existing.clone(),
            StorageItem::from_bytes(b"prefix".to_vec()),
        );
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let (overlay, capture) = prefix
            .transaction_overlay_with_dependency_capture(0, DependencyCaptureLimits::default())
            .expect("tracked overlay");
        let cache = overlay.snapshot_cache();

        cache.update(
            existing.clone(),
            StorageItem::from_bytes(b"overlay".to_vec()),
        );
        cache.add(
            created.clone(),
            StorageItem::from_bytes(b"created".to_vec()),
        );
        assert_eq!(
            cache.get(&existing).map(|item| item.to_value()),
            Some(b"overlay".to_vec())
        );
        assert_eq!(
            cache.get(&created).map(|item| item.to_value()),
            Some(b"created".to_vec())
        );

        assert!(
            capture
                .snapshot()
                .expect("overlay reads are supported")
                .point_reads()
                .is_empty()
        );
    }

    #[test]
    fn range_and_whole_store_reads_fail_closed() {
        let canonical = DataCache::new(false);
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let range_prefix = StorageKey::new(7, b"range".to_vec());
        let (range_overlay, range_capture) = prefix
            .transaction_overlay_with_dependency_capture(0, DependencyCaptureLimits::default())
            .expect("range overlay");
        range_overlay
            .snapshot_cache()
            .find(Some(&range_prefix), SeekDirection::Backward)
            .for_each(drop);
        assert_eq!(
            range_capture.snapshot(),
            Err(DependencyCaptureError::UnsupportedRangeRead {
                prefix: Some(range_prefix),
                direction: SeekDirection::Backward,
            })
        );

        let (whole_overlay, whole_capture) = prefix
            .transaction_overlay_with_dependency_capture(1, DependencyCaptureLimits::default())
            .expect("whole-store overlay");
        whole_overlay
            .snapshot_cache()
            .find(None, SeekDirection::Forward)
            .for_each(drop);
        assert_eq!(
            whole_capture.snapshot(),
            Err(DependencyCaptureError::UnsupportedRangeRead {
                prefix: None,
                direction: SeekDirection::Forward,
            })
        );
    }

    #[test]
    fn point_count_and_byte_limits_fail_closed() {
        let first = StorageKey::new(7, b"first".to_vec());
        let second = StorageKey::new(7, b"second".to_vec());
        let canonical = DataCache::new(false);
        canonical.add(first.clone(), StorageItem::from_bytes(vec![1]));
        canonical.add(second.clone(), StorageItem::from_bytes(vec![2]));
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let (count_overlay, count_capture) = prefix
            .transaction_overlay_with_dependency_capture(0, DependencyCaptureLimits::new(1, 100))
            .expect("count-bounded overlay");
        count_overlay.snapshot_cache().get(&first);
        count_overlay.snapshot_cache().get(&second);
        assert_eq!(
            count_capture.snapshot(),
            Err(DependencyCaptureError::PointReadLimitExceeded { limit: 1 })
        );

        let (byte_overlay, byte_capture) = prefix
            .transaction_overlay_with_dependency_capture(1, DependencyCaptureLimits::new(1, 0))
            .expect("byte-bounded overlay");
        byte_overlay.snapshot_cache().get(&first);
        assert!(matches!(
            byte_capture.snapshot(),
            Err(DependencyCaptureError::CapturedBytesLimitExceeded { limit: 0, .. })
        ));
    }

    #[test]
    fn ordinary_overlay_does_not_install_dependency_observation() {
        let canonical = DataCache::new(false);
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let overlay = prefix.transaction_overlay(0).expect("ordinary overlay");
        assert!(!overlay.snapshot_cache().has_read_observer());
    }

    #[test]
    fn dependency_observer_is_bound_before_engine_policy_initialization() {
        let exec_fee_key = DependencyPolicyProvider::key(DependencyPolicyProvider::EXEC_FEE_KEY);
        let storage_price_key =
            DependencyPolicyProvider::key(DependencyPolicyProvider::STORAGE_PRICE_KEY);
        let canonical = DataCache::new(false);
        canonical.add(
            exec_fee_key.clone(),
            StorageItem::from_bytes(30_u32.to_le_bytes().to_vec()),
        );
        canonical.add(
            storage_price_key.clone(),
            StorageItem::from_bytes(100_000_u32.to_le_bytes().to_vec()),
        );
        let prefix = PinnedBlockPrefix::capture(identity(0), &canonical);
        let (overlay, capture) = prefix
            .transaction_overlay_with_dependency_capture(0, DependencyCaptureLimits::default())
            .expect("tracked overlay");
        let mut block = Block::new();
        block.header.set_index(42);

        let _engine = ApplicationEngine::<DependencyPolicyProvider>::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            overlay.snapshot_cache(),
            Some(Arc::new(block)),
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(DependencyPolicyProvider),
        )
        .expect("engine");

        let dependencies = capture.snapshot().expect("constructor point reads");
        assert_eq!(dependencies.point_reads().len(), 2);
        assert!(
            dependencies
                .point_reads()
                .iter()
                .any(|dependency| dependency.key() == &exec_fee_key)
        );
        assert!(
            dependencies
                .point_reads()
                .iter()
                .any(|dependency| dependency.key() == &storage_price_key)
        );
    }
}
