use alloc::vec::Vec;

use neo_base::Bytes;
use neo_store::ColumnId;
use neo_vm::{StackItem, VmValue};

use super::ExecutionContext;
use crate::{
    error::ContractError,
    nef::CallFlags,
    runtime::storage::{StorageFindOptions, StorageFindOptionsError, StorageIterator},
};

impl<'a> ExecutionContext<'a> {
    pub fn load(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, ContractError> {
        self.require_call_flag(CallFlags::READ_STATES)?;
        self.store
            .get(column, key)
            .map_err(|_| ContractError::Storage("get failed"))
    }

    pub fn put(
        &mut self,
        column: ColumnId,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), ContractError> {
        self.require_call_flag(CallFlags::WRITE_STATES)?;
        self.store
            .put(column, key, value)
            .map_err(|_| ContractError::Storage("put failed"))
    }

    pub fn delete(&mut self, column: ColumnId, key: &[u8]) -> Result<(), ContractError> {
        self.require_call_flag(CallFlags::WRITE_STATES)?;
        self.store
            .delete(column, key)
            .map_err(|_| ContractError::Storage("delete failed"))
    }

    pub(crate) fn set_call_flags(&mut self, flags: CallFlags) {
        self.current_call_flags = flags;
    }

    pub(crate) fn call_flags(&self) -> CallFlags {
        self.current_call_flags
    }

    pub(crate) fn require_call_flag(&self, required: CallFlags) -> Result<(), ContractError> {
        if required == CallFlags::NONE || self.current_call_flags.contains(required) {
            Ok(())
        } else {
            Err(ContractError::MissingCallFlags(required))
        }
    }

    pub fn find_storage_items(
        &self,
        column: ColumnId,
        prefix: &[u8],
        options: StorageFindOptions,
    ) -> Result<Vec<VmValue>, ContractError> {
        self.require_call_flag(CallFlags::READ_STATES)?;
        Self::validate_options(options)?;

        let mut entries = self
            .store
            .scan_prefix(column, prefix)
            .map_err(|_| ContractError::Storage("scan prefix failed"))?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if options.contains(StorageFindOptions::BACKWARDS) {
            entries.reverse();
        }

        let mut items = Vec::with_capacity(entries.len());
        for (mut key, value) in entries {
            if options.contains(StorageFindOptions::REMOVE_PREFIX) && key.starts_with(prefix) {
                key.drain(..prefix.len());
            }
            let key_bytes = Bytes::from(key);
            let value_vm = self.decode_storage_value(value, options)?;
            let entry = if options.contains(StorageFindOptions::KEYS_ONLY) {
                VmValue::Bytes(key_bytes)
            } else if options.contains(StorageFindOptions::VALUES_ONLY) {
                value_vm
            } else {
                VmValue::Array(vec![VmValue::Bytes(key_bytes), value_vm])
            };
            items.push(entry);
        }
        Ok(items)
    }

    pub fn create_storage_iterator(
        &mut self,
        column: ColumnId,
        prefix: &[u8],
        options: StorageFindOptions,
    ) -> Result<u32, ContractError> {
        let items = self.find_storage_items(column, prefix, options)?;
        Ok(self.insert_storage_iterator(StorageIterator::new(items)))
    }

    pub fn create_storage_iterator_from_bits(
        &mut self,
        column: ColumnId,
        prefix: &[u8],
        options_bits: u8,
    ) -> Result<u32, ContractError> {
        let options = StorageFindOptions::from_bits(options_bits)
            .ok_or_else(|| ContractError::InvalidFindOptions("unknown flags".into()))?;
        self.create_storage_iterator(column, prefix, options)
    }

    pub fn storage_iterator_next(&mut self, handle: u32) -> Result<Option<VmValue>, ContractError> {
        if let Some(entry) = self.storage_iterators.get_mut(handle as usize) {
            if let Some(iterator) = entry {
                if let Some(item) = iterator.next() {
                    return Ok(Some(item));
                }
                *entry = None;
                Ok(None)
            } else {
                Ok(None)
            }
        } else {
            Err(ContractError::Runtime("invalid storage iterator handle"))
        }
    }

    fn insert_storage_iterator(&mut self, iterator: StorageIterator) -> u32 {
        if let Some((index, slot)) = self
            .storage_iterators
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.is_none())
        {
            *slot = Some(iterator);
            index as u32
        } else {
            self.storage_iterators.push(Some(iterator));
            (self.storage_iterators.len() - 1) as u32
        }
    }

    fn validate_options(options: StorageFindOptions) -> Result<(), ContractError> {
        options.validate().map_err(|err| match err {
            StorageFindOptionsError::UnknownFlags(_) => {
                ContractError::InvalidFindOptions("unknown flags".into())
            }
            StorageFindOptionsError::ConflictingKeysOnly => ContractError::InvalidFindOptions(
                "KeysOnly cannot be combined with value options".into(),
            ),
            StorageFindOptionsError::ConflictingValuesOnly => ContractError::InvalidFindOptions(
                "ValuesOnly cannot be combined with KeysOnly or RemovePrefix".into(),
            ),
            StorageFindOptionsError::ConflictingPickFields => ContractError::InvalidFindOptions(
                "PickField0 and PickField1 are mutually exclusive".into(),
            ),
            StorageFindOptionsError::PickFieldWithoutDeserialize => {
                ContractError::InvalidFindOptions("PickField requires DeserializeValues".into())
            }
        })
    }

    fn decode_storage_value(
        &self,
        value: Vec<u8>,
        options: StorageFindOptions,
    ) -> Result<VmValue, ContractError> {
        if options.contains(StorageFindOptions::DESERIALIZE_VALUES) {
            let mut item = StackItem::deserialize(&value).map_err(|_| {
                ContractError::InvalidFindOptions("failed to deserialize stack item".into())
            })?;
            if options.contains(StorageFindOptions::PICK_FIELD0) {
                item = Self::pick_field(item, 0)?;
            } else if options.contains(StorageFindOptions::PICK_FIELD1) {
                item = Self::pick_field(item, 1)?;
            }
            VmValue::try_from(item).map_err(|_| {
                ContractError::InvalidFindOptions("deserialized value not representable".into())
            })
        } else {
            Ok(VmValue::Bytes(Bytes::from(value)))
        }
    }

    fn pick_field(item: StackItem, index: usize) -> Result<StackItem, ContractError> {
        match item {
            StackItem::Array(values) | StackItem::Struct(values) => {
                values.get(index).cloned().ok_or_else(|| {
                    ContractError::InvalidFindOptions("PickField index out of bounds".into())
                })
            }
            _ => Err(ContractError::InvalidFindOptions(
                "PickField requires array or struct values".into(),
            )),
        }
    }
}
