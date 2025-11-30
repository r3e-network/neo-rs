use super::states::{
    BlockLogState, ContractLogState, EngineLogState, ExecutionLogState, NotifyLogState,
    TransactionEngineLogState, TransactionLogState,
};
use crate::application_logs::settings::ApplicationLogsSettings;
use neo_core::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_core::persistence::i_store_snapshot::IStoreSnapshot;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::TriggerType;
use neo_core::{UInt160, UInt256};
use neo_vm::{execution_engine_limits::ExecutionEngineLimits, StackItem};
use parking_lot::Mutex;
use std::sync::Arc;
use uuid::Uuid;

const PREFIX_SIZE: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();
const PREFIX_BLOCK_TRIGGER_SIZE: usize = PREFIX_SIZE + UInt256::LENGTH;
const PREFIX_EXECUTION_BLOCK_TRIGGER_SIZE: usize = PREFIX_SIZE + UInt256::LENGTH;
const CONTRACT_KEY_PREFIX_SIZE: usize = PREFIX_SIZE + UInt160::LENGTH;
const CONTRACT_TIMESTAMP_OFFSET: usize = CONTRACT_KEY_PREFIX_SIZE;
const CONTRACT_ITER_OFFSET: usize = CONTRACT_KEY_PREFIX_SIZE + std::mem::size_of::<u64>();

const PREFIX_ID: i32 = 0x414C4F47; // "ALOG"
const PREFIX_ENGINE: u8 = 0x18;
const PREFIX_ENGINE_TRANSACTION: u8 = 0x19;
const PREFIX_BLOCK: u8 = 0x20;
const PREFIX_NOTIFY: u8 = 0x21;
const PREFIX_CONTRACT: u8 = 0x22;
const PREFIX_EXECUTION: u8 = 0x23;
const PREFIX_EXECUTION_BLOCK: u8 = 0x24;
const PREFIX_EXECUTION_TRANSACTION: u8 = 0x25;
const PREFIX_TRANSACTION: u8 = 0x26;
const PREFIX_STACK_ITEM: u8 = 0xED;

const GUID_SIZE: usize = 16;

pub struct LogStorageStore {
    snapshot: SnapshotHandle,
}

#[derive(Clone, Debug)]
pub struct ContractLogRecord {
    pub state: ContractLogState,
    pub timestamp: u64,
    pub notification_index: u32,
}

impl LogStorageStore {
    pub fn new(snapshot: Arc<dyn IStoreSnapshot>) -> Self {
        Self {
            snapshot: SnapshotHandle::new(snapshot),
        }
    }

    pub fn put_engine_state(&mut self, state: &EngineLogState) -> IoResult<Uuid> {
        let id = Uuid::new_v4();
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_ENGINE)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)?;
        Ok(id)
    }

    pub fn put_transaction_engine_state(
        &mut self,
        hash: &UInt256,
        state: &TransactionEngineLogState,
    ) -> IoResult<()> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_ENGINE_TRANSACTION)
            .add_uint256(hash)
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)
    }

    pub fn put_block_state(
        &mut self,
        hash: &UInt256,
        trigger: TriggerType,
        state: &BlockLogState,
    ) -> IoResult<()> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_BLOCK)
            .add_uint256(hash)
            .add_u8(trigger.bits())
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)
    }

    pub fn put_notify_state(&mut self, state: &NotifyLogState) -> IoResult<Uuid> {
        let id = Uuid::new_v4();
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_NOTIFY)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)?;
        Ok(id)
    }

    pub fn put_contract_state(
        &mut self,
        script_hash: &UInt160,
        timestamp: u64,
        iter_index: u32,
        state: &ContractLogState,
    ) -> IoResult<()> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_CONTRACT)
            .add_uint160(script_hash)
            .add_be_u64(timestamp)
            .add_be_u32(iter_index)
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)
    }

    pub fn put_execution_state(&mut self, state: &ExecutionLogState) -> IoResult<Uuid> {
        let id = Uuid::new_v4();
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)?;
        Ok(id)
    }

    pub fn put_execution_block_state(
        &mut self,
        block_hash: &UInt256,
        trigger: TriggerType,
        execution_state_id: Uuid,
    ) -> IoResult<()> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION_BLOCK)
            .add_uint256(block_hash)
            .add_u8(trigger.bits())
            .into_vec();
        self.write_raw(key, execution_state_id.to_bytes_le().to_vec())
    }

    pub fn put_execution_transaction_state(
        &mut self,
        tx_hash: &UInt256,
        execution_state_id: Uuid,
    ) -> IoResult<()> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION_TRANSACTION)
            .add_uint256(tx_hash)
            .into_vec();
        self.write_raw(key, execution_state_id.to_bytes_le().to_vec())
    }

    pub fn put_transaction_state(
        &mut self,
        hash: &UInt256,
        state: &TransactionLogState,
    ) -> IoResult<()> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_TRANSACTION)
            .add_uint256(hash)
            .into_vec();
        let value = serialize_serializable(state)?;
        self.write_raw(key, value)
    }

    pub fn put_stack_item_state(&mut self, item: &StackItem) -> IoResult<Uuid> {
        let id = Uuid::new_v4();
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_STACK_ITEM)
            .add_bytes(&id.to_bytes_le())
            .into_vec();

        let mut limits = ExecutionEngineLimits::DEFAULT;
        limits.max_item_size = ApplicationLogsSettings::current().max_stack_size as u32;

        let data = BinarySerializer::serialize(item, &limits)
            .or_else(|_| BinarySerializer::serialize(&StackItem::null(), &limits))
            .map_err(IoError::invalid_data)?;

        self.write_raw(key, data)?;
        Ok(id)
    }

    pub fn find_block_state(&self, hash: &UInt256) -> IoResult<Vec<(BlockLogState, TriggerType)>> {
        let prefix_bytes = LogKeyBuilder::new(PREFIX_ID, PREFIX_BLOCK)
            .add_uint256(hash)
            .into_vec();

        self.snapshot.with(|snapshot| {
            let iter = snapshot.find(Some(&prefix_bytes), SeekDirection::Forward);
            let mut results = Vec::new();
            for (key, value) in iter {
                if !key.starts_with(&prefix_bytes) {
                    break;
                }
                if key.len() <= PREFIX_BLOCK_TRIGGER_SIZE {
                    continue;
                }
                let trigger = TriggerType::from_bits(key[PREFIX_BLOCK_TRIGGER_SIZE])
                    .ok_or_else(|| IoError::invalid_data("Invalid trigger type"))?;
                let state = deserialize_serializable::<BlockLogState>(&value)?;
                results.push((state, trigger));
            }
            Ok(results)
        })
    }

    pub fn find_contract_state(
        &self,
        script_hash: &UInt160,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogRecord>> {
        self.find_contract_state_internal(script_hash, None, None, page, page_size)
    }

    pub fn find_contract_state_with_trigger(
        &self,
        script_hash: &UInt160,
        trigger: TriggerType,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogRecord>> {
        self.find_contract_state_internal(script_hash, Some(trigger), None, page, page_size)
    }

    pub fn find_contract_state_with_trigger_and_event(
        &self,
        script_hash: &UInt160,
        trigger: TriggerType,
        event_name: &str,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogRecord>> {
        self.find_contract_state_internal(
            script_hash,
            Some(trigger),
            Some(event_name),
            page,
            page_size,
        )
    }

    pub fn find_execution_block_state(&self, hash: &UInt256) -> IoResult<Vec<(Uuid, TriggerType)>> {
        let prefix_bytes = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION_BLOCK)
            .add_uint256(hash)
            .into_vec();

        self.snapshot.with(|snapshot| {
            let iter = snapshot.find(Some(&prefix_bytes), SeekDirection::Forward);
            let mut results = Vec::new();
            for (key, value) in iter {
                if !key.starts_with(&prefix_bytes) {
                    break;
                }
                if key.len() <= PREFIX_EXECUTION_BLOCK_TRIGGER_SIZE || value.len() != GUID_SIZE {
                    continue;
                }
                let trigger = TriggerType::from_bits(key[PREFIX_EXECUTION_BLOCK_TRIGGER_SIZE])
                    .ok_or_else(|| IoError::invalid_data("Invalid trigger type"))?;
                let mut guid_bytes = [0u8; GUID_SIZE];
                guid_bytes.copy_from_slice(&value);
                results.push((Uuid::from_bytes_le(guid_bytes), trigger));
            }
            Ok(results)
        })
    }

    pub fn try_get_engine_state(&self, id: Uuid) -> IoResult<Option<EngineLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_ENGINE)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        self.read_serializable::<EngineLogState>(&key)
    }

    pub fn try_get_transaction_engine_state(
        &self,
        hash: &UInt256,
    ) -> IoResult<Option<TransactionEngineLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_ENGINE_TRANSACTION)
            .add_uint256(hash)
            .into_vec();
        self.read_serializable::<TransactionEngineLogState>(&key)
    }

    pub fn try_get_block_state(
        &self,
        hash: &UInt256,
        trigger: TriggerType,
    ) -> IoResult<Option<BlockLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_BLOCK)
            .add_uint256(hash)
            .add_u8(trigger.bits())
            .into_vec();
        self.read_serializable::<BlockLogState>(&key)
    }

    pub fn try_get_notify_state(&self, id: Uuid) -> IoResult<Option<NotifyLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_NOTIFY)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        self.read_serializable::<NotifyLogState>(&key)
    }

    pub fn try_get_contract_state(
        &self,
        script_hash: &UInt160,
        timestamp: u64,
        iter_index: u32,
    ) -> IoResult<Option<ContractLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_CONTRACT)
            .add_uint160(script_hash)
            .add_be_u64(timestamp)
            .add_be_u32(iter_index)
            .into_vec();
        self.read_serializable::<ContractLogState>(&key)
    }

    pub fn try_get_execution_state(&self, id: Uuid) -> IoResult<Option<ExecutionLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        self.read_serializable::<ExecutionLogState>(&key)
    }

    pub fn try_get_execution_block_state(
        &self,
        block_hash: &UInt256,
        trigger: TriggerType,
    ) -> IoResult<Option<Uuid>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION_BLOCK)
            .add_uint256(block_hash)
            .add_u8(trigger.bits())
            .into_vec();
        if let Some(bytes) = self.read_raw(&key) {
            if bytes.len() == GUID_SIZE {
                let mut buffer = [0u8; GUID_SIZE];
                buffer.copy_from_slice(&bytes);
                Ok(Some(Uuid::from_bytes_le(buffer)))
            } else {
                Err(IoError::invalid_data("Invalid GUID length"))
            }
        } else {
            Ok(None)
        }
    }

    pub fn try_get_execution_transaction_state(&self, hash: &UInt256) -> IoResult<Option<Uuid>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_EXECUTION_TRANSACTION)
            .add_uint256(hash)
            .into_vec();
        if let Some(bytes) = self.read_raw(&key) {
            if bytes.len() == GUID_SIZE {
                let mut buffer = [0u8; GUID_SIZE];
                buffer.copy_from_slice(&bytes);
                Ok(Some(Uuid::from_bytes_le(buffer)))
            } else {
                Err(IoError::invalid_data("Invalid GUID length"))
            }
        } else {
            Ok(None)
        }
    }

    pub fn try_get_transaction_state(
        &self,
        hash: &UInt256,
    ) -> IoResult<Option<TransactionLogState>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_TRANSACTION)
            .add_uint256(hash)
            .into_vec();
        self.read_serializable::<TransactionLogState>(&key)
    }

    pub fn try_get_stack_item_state(&self, id: Uuid) -> IoResult<Option<StackItem>> {
        let key = LogKeyBuilder::new(PREFIX_ID, PREFIX_STACK_ITEM)
            .add_bytes(&id.to_bytes_le())
            .into_vec();
        if let Some(bytes) = self.read_raw(&key) {
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::DEFAULT, None)
                .map(Some)
                .map_err(IoError::invalid_data)
        } else {
            Ok(None)
        }
    }

    pub fn commit(&mut self) {
        self.snapshot.with_mut(|snapshot| snapshot.commit());
    }

    fn find_contract_state_internal(
        &self,
        script_hash: &UInt160,
        trigger: Option<TriggerType>,
        event_name: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogRecord>> {
        let base_prefix = LogKeyBuilder::new(PREFIX_ID, PREFIX_CONTRACT)
            .add_uint160(script_hash)
            .into_vec();

        let seek_bytes = LogKeyBuilder::new(PREFIX_ID, PREFIX_CONTRACT)
            .add_uint160(script_hash)
            .add_be_u64(u64::MAX)
            .into_vec();

        self.snapshot.with(|snapshot| {
            let iter = snapshot.find(Some(&seek_bytes), SeekDirection::Backward);
            let mut collected = Vec::new();
            let mut index = 1u32;

            for (key, value) in iter {
                if !key.starts_with(&base_prefix) {
                    break;
                }

                let state = deserialize_serializable::<ContractLogState>(&value)?;
                if key.len() < CONTRACT_ITER_OFFSET + std::mem::size_of::<u32>() {
                    continue;
                }
                let mut timestamp_bytes = [0u8; 8];
                timestamp_bytes.copy_from_slice(
                    &key[CONTRACT_TIMESTAMP_OFFSET..CONTRACT_TIMESTAMP_OFFSET + 8],
                );
                let timestamp = u64::from_be_bytes(timestamp_bytes);

                let mut iter_bytes = [0u8; 4];
                iter_bytes.copy_from_slice(&key[CONTRACT_ITER_OFFSET..CONTRACT_ITER_OFFSET + 4]);
                let notification_index = u32::from_be_bytes(iter_bytes);

                if let Some(expected_trigger) = trigger {
                    if state.trigger != expected_trigger {
                        continue;
                    }
                }

                if let Some(expected_event) = event_name {
                    if !state.notify.event_name.eq_ignore_ascii_case(expected_event) {
                        continue;
                    }
                }

                if index >= page && index < page + page_size {
                    collected.push(ContractLogRecord {
                        state,
                        timestamp,
                        notification_index,
                    });
                }
                index += 1;
            }

            Ok(collected)
        })
    }

    fn write_raw(&mut self, key: Vec<u8>, value: Vec<u8>) -> IoResult<()> {
        self.snapshot.with_mut(|snapshot| snapshot.put(key, value));
        Ok(())
    }

    fn read_serializable<T: Serializable>(&self, key: &[u8]) -> IoResult<Option<T>> {
        if let Some(bytes) = self.read_raw(key) {
            let value = deserialize_serializable::<T>(&bytes)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn read_raw(&self, key: &[u8]) -> Option<Vec<u8>> {
        let key_vec = key.to_vec();
        self.snapshot.with(|snapshot| snapshot.try_get(&key_vec))
    }
}

struct LogKeyBuilder {
    buffer: Vec<u8>,
}

impl LogKeyBuilder {
    fn new(id: i32, prefix: u8) -> Self {
        let mut buffer = Vec::with_capacity(64);
        buffer.extend_from_slice(&id.to_le_bytes());
        buffer.push(prefix);
        Self { buffer }
    }

    fn add_bytes(mut self, bytes: &[u8]) -> Self {
        self.buffer.extend_from_slice(bytes);
        self
    }

    fn add_u8(mut self, value: u8) -> Self {
        self.buffer.push(value);
        self
    }

    fn add_uint160(mut self, value: &UInt160) -> Self {
        self.buffer.extend_from_slice(&value.to_bytes());
        self
    }

    fn add_uint256(mut self, value: &UInt256) -> Self {
        self.buffer.extend_from_slice(&value.to_bytes());
        self
    }

    fn add_be_u64(mut self, value: u64) -> Self {
        self.buffer.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn add_be_u32(mut self, value: u32) -> Self {
        self.buffer.extend_from_slice(&value.to_be_bytes());
        self
    }

    fn into_vec(self) -> Vec<u8> {
        self.buffer
    }
}

enum SnapshotHandle {
    Owned(Box<dyn IStoreSnapshot>),
    Shared {
        snapshot: Arc<dyn IStoreSnapshot>,
        lock: Mutex<()>,
    },
}

impl SnapshotHandle {
    fn new(snapshot: Arc<dyn IStoreSnapshot>) -> Self {
        if Arc::strong_count(&snapshot) == 1 {
            let raw = Arc::into_raw(snapshot);
            let boxed = unsafe { Box::from_raw(raw as *mut dyn IStoreSnapshot) };
            SnapshotHandle::Owned(boxed)
        } else {
            SnapshotHandle::Shared {
                snapshot,
                lock: Mutex::new(()),
            }
        }
    }

    fn with_mut<R>(&mut self, f: impl FnOnce(&mut dyn IStoreSnapshot) -> R) -> R {
        match self {
            SnapshotHandle::Owned(inner) => f(inner.as_mut()),
            SnapshotHandle::Shared { snapshot, lock } => {
                let _guard = lock.lock();
                let raw = Arc::as_ptr(snapshot) as *mut dyn IStoreSnapshot;
                unsafe { f(&mut *raw) }
            }
        }
    }

    fn with<R>(&self, f: impl FnOnce(&dyn IStoreSnapshot) -> R) -> R {
        match self {
            SnapshotHandle::Owned(inner) => f(inner.as_ref()),
            SnapshotHandle::Shared { snapshot, .. } => f(snapshot.as_ref()),
        }
    }
}

fn serialize_serializable<T: Serializable>(value: &T) -> IoResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    value.serialize(&mut writer)?;
    Ok(writer.into_bytes())
}

fn deserialize_serializable<T: Serializable>(bytes: &[u8]) -> IoResult<T> {
    let mut reader = MemoryReader::new(bytes);
    T::deserialize(&mut reader)
}
