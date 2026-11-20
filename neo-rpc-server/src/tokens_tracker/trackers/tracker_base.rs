//! Shared base for token trackers.
//!
//! Provides common functionality for NEP-11 and NEP-17 trackers including
//! database operations and transfer record extraction.

use super::token_transfer::TokenTransfer;
use super::token_transfer_key::TokenTransferKey;
use neo_io::{MemoryReader, Serializable, SerializableExtensions as SerializableExt};
use neo_ledger::ApplicationExecuted;
use neo_native_contracts::native::contract_management::ContractManagement;
use neo_primitives::CallFlags;
use neo_primitives::TriggerType;
use neo_primitives::UInt160;
use neo_primitives::UInt256;
use neo_primitives::Verifiable;
use neo_primitives::log_level::LogLevel;
use neo_protocol::execution::Block;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_storage::persistence::{DataCache, SeekDirection, Store, StoreSnapshot};
use neo_system::NeoSystem;
use neo_vm::OpCode;
use neo_vm::ScriptBuilder;
use neo_vm::StackItem;
use neo_vm::VmState;
use num_bigint::BigInt;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Transfer record extracted from a `Transfer` notification.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferRecord {
    /// Contract script hash (asset).
    pub asset: UInt160,
    /// Sender address (zero for mint).
    pub from: UInt160,
    /// Recipient address (zero for burn).
    pub to: UInt160,
    /// Token ID for NEP-11 transfers.
    pub token_id: Option<Vec<u8>>,
    /// Transfer amount.
    pub amount: BigInt,
}

/// One `System.Contract.Call` to run through [`TrackerBase::call_contract`].
pub struct ContractCall<'a> {
    /// Method to call on the asset contract.
    pub method: &'a str,
    /// Arguments in call order, already serialized as byte strings.
    pub args: Vec<Vec<u8>>,
}

/// Common view over transfer-key types (TokenTransferKey, Nep17TransferKey, Nep11TransferKey).
pub trait TokenTransferKeyView {
    /// Returns the user's script hash.
    fn user_script_hash(&self) -> &UInt160;
    /// Returns the timestamp in milliseconds.
    fn timestamp_ms(&self) -> u64;
    /// Returns the asset's script hash.
    fn asset_script_hash(&self) -> &UInt160;
    /// Returns the notification index within the block.
    fn block_xfer_notification_index(&self) -> u32;
}

impl<T> TokenTransferKeyView for T
where
    T: AsRef<TokenTransferKey>,
{
    fn user_script_hash(&self) -> &UInt160 {
        &self.as_ref().user_script_hash
    }

    fn timestamp_ms(&self) -> u64 {
        self.as_ref().timestamp_ms
    }

    fn asset_script_hash(&self) -> &UInt160 {
        &self.as_ref().asset_script_hash
    }

    fn block_xfer_notification_index(&self) -> u32 {
        self.as_ref().block_xfer_notification_index
    }
}

/// Trait implemented by NEP-11/NEP-17 trackers.
pub trait Tracker: Send + Sync {
    /// Returns the tracker's name for logging.
    fn track_name(&self) -> &str;

    /// Called when a block is being persisted.
    fn on_persist(
        &mut self,
        system: &NeoSystem,
        block: &Block,
        snapshot: &DataCache,
        executed_list: &[ApplicationExecuted],
    );

    /// Resets the current batch (starts a new snapshot).
    fn reset_batch(&mut self);

    /// Commits the current batch to the database.
    fn commit(&mut self) -> Result<(), String>;
}

/// Base tracker state shared by NEP-11 and NEP-17 trackers.
pub struct TrackerBase {
    /// Whether to track transfer history.
    pub should_track_history: bool,
    /// Maximum results for queries.
    pub max_results: u32,
    /// Database store.
    pub db: Arc<dyn Store>,
    /// Current snapshot for batch operations.
    snapshot: Option<Arc<dyn StoreSnapshot>>,
    /// Reference to the Neo system.
    pub neo_system: Arc<NeoSystem>,
}

impl TrackerBase {
    /// Creates a new TrackerBase.
    pub fn new(
        db: Arc<dyn Store>,
        max_results: u32,
        should_track_history: bool,
        neo_system: Arc<NeoSystem>,
    ) -> Self {
        Self {
            should_track_history,
            max_results,
            db,
            snapshot: None,
            neo_system,
        }
    }

    /// Resets the batch by creating a new snapshot.
    pub fn reset_batch(&mut self) {
        self.snapshot = Some(self.db.snapshot());
    }

    /// Commits the current snapshot to the database.
    pub fn commit(&mut self) -> Result<(), String> {
        let Some(snapshot_arc) = self.snapshot.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err("snapshot commit failed: snapshot is still shared".to_string());
        };
        snapshot
            .try_commit()
            .map_err(|e| format!("snapshot commit failed: {e}"))?;
        Ok(())
    }

    fn key<K: Serializable>(prefix: u8, key: &K) -> Result<Vec<u8>, String> {
        let mut buffer = Vec::with_capacity(key.size() + 1);
        buffer.push(prefix);
        buffer.extend_from_slice(&key.to_array().map_err(|e| e.to_string())?);
        Ok(buffer)
    }

    /// Stores a key-value pair with the given prefix.
    pub fn put<K: Serializable, V: Serializable>(
        &mut self,
        prefix: u8,
        key: &K,
        value: &V,
    ) -> Result<(), String> {
        let Some(snapshot_arc) = self.snapshot.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Ok(());
        };
        let key_bytes = Self::key(prefix, key)?;
        let value_bytes = value.to_array().map_err(|e| e.to_string())?;
        snapshot
            .put(key_bytes, value_bytes)
            .map_err(|e| format!("storage put failed: {e}"))?;
        Ok(())
    }

    /// Deletes a key with the given prefix.
    pub fn delete<K: Serializable>(&mut self, prefix: u8, key: &K) -> Result<(), String> {
        let Some(snapshot_arc) = self.snapshot.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Ok(());
        };
        let key_bytes = Self::key(prefix, key)?;
        snapshot
            .delete(key_bytes)
            .map_err(|e| format!("storage delete failed: {e}"))?;
        Ok(())
    }

    /// Queries transfers within a time range.
    pub fn query_transfers<TKey, TValue>(
        &self,
        db_prefix: u8,
        user_script_hash: &UInt160,
        start_time: u64,
        end_time: u64,
    ) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: Serializable,
        TValue: Serializable,
    {
        let mut prefix_bytes = vec![db_prefix];
        prefix_bytes.extend_from_slice(&user_script_hash.to_bytes());

        let start_key = [prefix_bytes.as_slice(), &start_time.to_be_bytes()].concat();
        let end_key = [prefix_bytes.as_slice(), &end_time.to_be_bytes()].concat();

        let start_vec = start_key.clone();
        let mut results = Vec::new();

        let snapshot = self.db.snapshot();
        for (key_bytes, value_bytes) in snapshot.find(Some(&start_vec), SeekDirection::Forward) {
            if key_bytes.as_slice() > end_key.as_slice() {
                break;
            }
            if !key_bytes.starts_with(&prefix_bytes) {
                break;
            }

            let mut key_reader = MemoryReader::new(&key_bytes[1..]);
            let key = TKey::deserialize(&mut key_reader).map_err(|e| e.to_string())?;

            let mut val_reader = MemoryReader::new(&value_bytes);
            let val = TValue::deserialize(&mut val_reader).map_err(|e| e.to_string())?;

            results.push((key, val));
        }
        Ok(results)
    }

    /// Builds the script that makes every call in `calls` against `asset`.
    ///
    /// Each call is emitted as its packed arguments, the call flags, the method
    /// name, the target contract and `System.Contract.Call`, one after another,
    /// so a single execution answers all of them in order. Returns `None` when a
    /// syscall cannot be encoded.
    pub fn call_script(asset: &UInt160, calls: &[ContractCall<'_>]) -> Option<Vec<u8>> {
        let mut builder = ScriptBuilder::new();
        for call in calls {
            for arg in &call.args {
                builder.emit_push(arg);
            }
            builder.emit_push_int(call.args.len() as i64);
            builder.emit_opcode(OpCode::PACK);
            builder.emit_push_int(CallFlags::ALL.bits() as i64);
            builder.emit_push(call.method.as_bytes());
            builder.emit_push(&asset.to_bytes());
            builder.emit_syscall("System.Contract.Call").ok()?;
        }
        Some(builder.to_array())
    }

    /// Runs `calls` against `asset` and returns the engine holding the answers.
    ///
    /// Both trackers keep balances current by re-executing `balanceOf` on the
    /// token contract. Building the script, configuring the engine and treating
    /// a failed call as no answer at all is the same work for both of them; only
    /// the method, its arguments, the gas budget and how many answers to expect
    /// differ per standard. Taking a list of calls is what lets NEP-11 ask for
    /// the sender and the recipient in one script and still pay for a single
    /// engine setup per transfer.
    ///
    /// The answers sit on the result stack in call order — the first call's
    /// answer deepest — and stay unread; which of them means what belongs to the
    /// caller, as does the reporting of a `None`.
    pub fn call_contract(
        &self,
        snapshot: &DataCache,
        current_block: Option<&Block>,
        asset: UInt160,
        gas_limit: i64,
        calls: &[ContractCall<'_>],
    ) -> Option<ApplicationEngine> {
        let script = Self::call_script(&asset, calls)?;
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(snapshot.clone()),
            current_block
                .cloned()
                .map(neo_protocol::execution::conversions::convert_ledger_block),
            self.neo_system.settings().clone(),
            gas_limit,
            None,
            neo_native_contracts::native::composition::default_native_services(),
        )
        .ok()?;
        engine
            .load_script(script, CallFlags::ALL, Some(asset))
            .and_then(|_| engine.execute())
            .ok()?;

        Some(engine)
    }

    /// Extracts a transfer record from a `Transfer` notification payload.
    pub fn get_transfer_record(
        asset: &UInt160,
        state_items: &[StackItem],
    ) -> Option<TransferRecord> {
        if state_items.len() < 3 {
            return None;
        }

        let from_item = &state_items[0];
        let to_item = &state_items[1];
        let amount_item = &state_items[2];

        if !from_item.is_null() && !matches!(from_item, StackItem::ByteString(_)) {
            return None;
        }
        if !to_item.is_null() && !matches!(to_item, StackItem::ByteString(_)) {
            return None;
        }
        if !matches!(
            amount_item,
            StackItem::ByteString(_) | StackItem::Integer(_)
        ) {
            return None;
        }

        let from_bytes = if from_item.is_null() {
            None
        } else {
            Some(from_item.as_bytes().ok()?)
        };
        if let Some(ref bytes) = from_bytes
            && bytes.len() != 20
        {
            return None;
        }

        let to_bytes = if to_item.is_null() {
            None
        } else {
            Some(to_item.as_bytes().ok()?)
        };
        if let Some(ref bytes) = to_bytes
            && bytes.len() != 20
        {
            return None;
        }

        if from_bytes.is_none() && to_bytes.is_none() {
            return None;
        }

        let from = from_bytes
            .as_ref()
            .and_then(|b| UInt160::from_bytes(b).ok())
            .unwrap_or_else(UInt160::zero);
        let to = to_bytes
            .as_ref()
            .and_then(|b| UInt160::from_bytes(b).ok())
            .unwrap_or_else(UInt160::zero);

        let amount = amount_item.as_integer().ok()?;

        let token_id = if state_items.len() == 4 {
            match &state_items[3] {
                StackItem::ByteString(bytes) => Some(bytes.clone()),
                _ => None,
            }
        } else {
            None
        };

        Some(TransferRecord {
            asset: *asset,
            from,
            to,
            token_id,
            amount,
        })
    }

    /// Visits every transfer notification in `executed_list` that belongs to a
    /// contract advertising `standard`.
    ///
    /// Both trackers walk the same gauntlet before they can use a record — skip
    /// faulted executions, keep `Transfer` events carrying exactly
    /// `expected_state_items` arguments, resolve the emitting contract, check its
    /// manifest — and only diverge on what the record means. The gauntlet lives
    /// here so the two trackers cannot drift apart.
    ///
    /// `visit` receives the record, the transaction that emitted it (absent for
    /// block-level notifications) and the running notification index.
    pub fn for_each_transfer(
        snapshot: &DataCache,
        executed_list: &[ApplicationExecuted],
        standard: &str,
        expected_state_items: usize,
        mut visit: impl FnMut(&TransferRecord, Option<&Arc<dyn Verifiable>>, &mut u32),
    ) {
        let mut transfer_index: u32 = 0;
        for app in executed_list {
            if app.vm_state.contains(VmState::FAULT) {
                continue;
            }
            for notify in &app.notifications {
                if notify.event_name != "Transfer" || notify.state.len() != expected_state_items {
                    continue;
                }

                let Ok(Some(contract)) =
                    ContractManagement::get_contract_from_snapshot(snapshot, &notify.script_hash)
                else {
                    continue;
                };

                if !contract.manifest.supports_standard(standard) {
                    continue;
                }

                let Some(record) = Self::get_transfer_record(&notify.script_hash, &notify.state)
                else {
                    continue;
                };

                visit(
                    &record,
                    notify.script_container.as_ref(),
                    &mut transfer_index,
                );
            }
        }
    }

    /// Writes both sides of a transfer (sent and received) into the batch.
    ///
    /// NEP-11 and NEP-17 store the same [`TokenTransfer`] payload and differ only
    /// in the key layout, so the key is built by `build_key` from the user and
    /// the notification index.
    #[allow(clippy::too_many_arguments)]
    pub fn record_transfer_history<K: Serializable>(
        &mut self,
        sent_prefix: u8,
        received_prefix: u8,
        record: &TransferRecord,
        block_index: u32,
        tx_hash: UInt256,
        transfer_index: u32,
        build_key: impl Fn(UInt160, u32) -> K,
    ) {
        if record.from != UInt160::zero() {
            let key = build_key(record.from, transfer_index);
            let value = TokenTransfer {
                amount: record.amount.clone(),
                user_script_hash: record.to,
                block_index,
                tx_hash,
            };
            let _ = self.put(sent_prefix, &key, &value);
        }

        if record.to != UInt160::zero() {
            let key = build_key(record.to, transfer_index);
            let value = TokenTransfer {
                amount: record.amount.clone(),
                user_script_hash: record.from,
                block_index,
                tx_hash,
            };
            let _ = self.put(received_prefix, &key, &value);
        }
    }

    /// Base JSON conversion for a transfer pair (without NEP-11 tokenid).
    pub fn transfer_to_json<K: TokenTransferKeyView>(
        &self,
        key: &K,
        value: &TokenTransfer,
    ) -> Value {
        let transfer_address = if value.user_script_hash == UInt160::zero() {
            Value::Null
        } else {
            Value::String(value.user_script_hash.to_address())
        };
        json!({
            "timestamp": key.timestamp_ms(),
            "assethash": key.asset_script_hash().to_string(),
            "transferaddress": transfer_address,
            "amount": value.amount.to_string(),
            "blockindex": value.block_index,
            "transfernotifyindex": key.block_xfer_notification_index(),
            "txhash": value.tx_hash.to_string(),
        })
    }

    /// Logs a message with the tracker name.
    pub fn log(track: &str, message: &str, level: LogLevel) {
        match level {
            LogLevel::Debug => debug!(target: "neo::tokens_tracker", track, message),
            LogLevel::Info => info!(target: "neo::tokens_tracker", track, message),
            LogLevel::Warning => warn!(target: "neo::tokens_tracker", track, message),
            LogLevel::Error | LogLevel::Fatal => {
                error!(target: "neo::tokens_tracker", track, message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_protocol::ProtocolSettings;
    use neo_storage::persistence::{
        read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
        storage::StorageError,
        store::OnNewSnapshotDelegate,
        write_store::WriteStore,
    };
    use neo_storage::{StorageItem, StorageKey};
    use std::any::Any;

    #[derive(Clone)]
    struct FailingStore;

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingStore {
        fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&Vec<u8>>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FailingStore {
        fn try_get(&self, _key: &StorageKey) -> Option<StorageItem> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&StorageKey>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl ReadOnlyStore for FailingStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingStore {
        fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }
    }

    impl Store for FailingStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            Arc::new(FailingSnapshot {
                store: Arc::new(self.clone()),
            })
        }

        fn on_new_snapshot(&self, _handler: OnNewSnapshotDelegate) {}

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FailingSnapshot {
        store: Arc<dyn Store>,
    }

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
            None
        }

        fn find(
            &self,
            _key_prefix: Option<&Vec<u8>>,
            _direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            Box::new(std::iter::empty())
        }
    }

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }
    }

    impl StoreSnapshot for FailingSnapshot {
        fn store(&self) -> Arc<dyn Store> {
            Arc::clone(&self.store)
        }

        fn try_commit(&mut self) -> neo_storage::persistence::store_snapshot::SnapshotCommitResult {
            Err(StorageError::CommitFailed(
                "injected tracker commit failure".to_string(),
            ))
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tracker_base_commit_propagates_snapshot_try_commit_failure() {
        let system = NeoSystem::new(ProtocolSettings::mainnet(), None, None).expect("neo system");
        let mut tracker = TrackerBase::new(Arc::new(FailingStore), 100, true, system);
        tracker.reset_batch();

        let err = tracker
            .commit()
            .expect_err("tracker commit should propagate snapshot commit failure");

        assert!(err.contains("injected tracker commit failure"));
    }

    fn balance_of_script(asset: &UInt160, caller: [u8; 20]) -> Vec<u8> {
        TrackerBase::call_script(
            asset,
            &[ContractCall {
                method: "balanceOf",
                args: vec![caller.to_vec()],
            }],
        )
        .expect("balanceOf script")
    }

    /// Pins the exact script the trackers execute against token contracts, so a
    /// change to the emitter cannot silently alter the queries.
    #[test]
    fn call_script_emits_one_packed_contract_call() {
        let asset = UInt160::zero();
        let script = balance_of_script(&asset, [0xAA; 20]);

        let mut expected = vec![OpCode::PUSHDATA1.byte(), 20];
        expected.extend_from_slice(&[0xAA; 20]);
        expected.push(OpCode::PUSH1.byte());
        expected.push(OpCode::PACK.byte());
        expected.push(OpCode::PUSH15.byte());
        expected.push(OpCode::PUSHDATA1.byte());
        expected.push(b"balanceOf".len() as u8);
        expected.extend_from_slice(b"balanceOf");
        expected.push(OpCode::PUSHDATA1.byte());
        expected.push(20);
        expected.extend_from_slice(&asset.to_bytes());
        expected.push(OpCode::SYSCALL.byte());
        expected.extend_from_slice(&neo_vm::interop_hash("System.Contract.Call").to_le_bytes());

        assert_eq!(script, expected);
    }

    #[test]
    fn call_script_chains_calls_in_order() {
        let asset = UInt160::zero();
        let first = balance_of_script(&asset, [0xAA; 20]);
        let second = balance_of_script(&asset, [0xBB; 20]);
        let both = TrackerBase::call_script(
            &asset,
            &[
                ContractCall {
                    method: "balanceOf",
                    args: vec![vec![0xAA; 20]],
                },
                ContractCall {
                    method: "balanceOf",
                    args: vec![vec![0xBB; 20]],
                },
            ],
        )
        .expect("chained balanceOf script");

        assert_eq!(both, [first, second].concat());
    }
}
