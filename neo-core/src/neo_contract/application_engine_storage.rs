// Copyright (C) 2015-2024 The Neo Project.
//
// application_engine.storage.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::*;
use neo::storage::{StorageContext, StorageKey, StorageItem};
use neo::vm::InteropDescriptor;
use neo::vm::CallFlags;
use neo::sys;
use neo::io::*;

/// The maximum size of storage keys.
pub const MAX_STORAGE_KEY_SIZE: usize = 64;

/// The maximum size of storage values.
pub const MAX_STORAGE_VALUE_SIZE: usize = u16::MAX as usize;

pub struct ApplicationEngine {
    // Other fields...
}

impl ApplicationEngine {
    /// The `InteropDescriptor` of System.Storage.GetContext.
    /// Gets the storage context for the current contract.
    pub static SYSTEM_STORAGE_GET_CONTEXT: InteropDescriptor = register_interop(
        "System.Storage.GetContext",
        ApplicationEngine::get_storage_context,
        1 << 4,
        CallFlags::READ_STATES,
    );

    /// The `InteropDescriptor` of System.Storage.GetReadOnlyContext.
    /// Gets the readonly storage context for the current contract.
    pub static SYSTEM_STORAGE_GET_READ_ONLY_CONTEXT: InteropDescriptor = register_interop(
        "System.Storage.GetReadOnlyContext",
        ApplicationEngine::get_read_only_context,
        1 << 4,
        CallFlags::READ_STATES,
    );

    /// The `InteropDescriptor` of System.Storage.AsReadOnly.
    /// Converts the specified storage context to a new readonly storage context.
    pub static SYSTEM_STORAGE_AS_READ_ONLY: InteropDescriptor = register_interop(
        "System.Storage.AsReadOnly",
        ApplicationEngine::as_read_only,
        1 << 4,
        CallFlags::READ_STATES,
    );

    /// The `InteropDescriptor` of System.Storage.Get.
    /// Gets the entry with the specified key from the storage.
    pub static SYSTEM_STORAGE_GET: InteropDescriptor = register_interop(
        "System.Storage.Get",
        ApplicationEngine::get,
        1 << 15,
        CallFlags::READ_STATES,
    );

    /// The `InteropDescriptor` of System.Storage.Find.
    /// Finds the entries from the storage.
    pub static SYSTEM_STORAGE_FIND: InteropDescriptor = register_interop(
        "System.Storage.Find",
        ApplicationEngine::find,
        1 << 15,
        CallFlags::READ_STATES,
    );

    /// The `InteropDescriptor` of System.Storage.Put.
    /// Puts a new entry into the storage.
    pub static SYSTEM_STORAGE_PUT: InteropDescriptor = register_interop(
        "System.Storage.Put",
        ApplicationEngine::put,
        1 << 15,
        CallFlags::WRITE_STATES,
    );

    /// The `InteropDescriptor` of System.Storage.Delete.
    /// Deletes an entry from the storage.
    pub static SYSTEM_STORAGE_DELETE: InteropDescriptor = register_interop(
        "System.Storage.Delete",
        ApplicationEngine::delete,
        1 << 15,
        CallFlags::WRITE_STATES,
    );

    /// The implementation of System.Storage.GetContext.
    /// Gets the storage context for the current contract.
    fn get_storage_context(&self) -> StorageContext {
        let contract = self.snapshot_cache.get_contract(&self.current_script_hash)
            .expect("Contract not found");
        StorageContext {
            id: contract.id,
            is_read_only: false,
        }
    }

    /// The implementation of System.Storage.GetReadOnlyContext.
    /// Gets the readonly storage context for the current contract.
    fn get_read_only_context(&self) -> StorageContext {
        let contract = self.snapshot_cache.get_contract(&self.current_script_hash)
            .expect("Contract not found");
        StorageContext {
            id: contract.id,
            is_read_only: true,
        }
    }

    /// The implementation of System.Storage.AsReadOnly.
    /// Converts the specified storage context to a new readonly storage context.
    fn as_read_only(context: StorageContext) -> StorageContext {
        if !context.is_read_only {
            StorageContext {
                id: context.id,
                is_read_only: true,
            }
        } else {
            context
        }
    }

    /// The implementation of System.Storage.Get.
    /// Gets the entry with the specified key from the storage.
    fn get(&self, context: &StorageContext, key: &[u8]) -> Option<Vec<u8>> {
        let storage_key = StorageKey {
            id: context.id,
            key: key.to_vec(),
        };
        self.snapshot_cache.get(&storage_key).map(|item| item.value.clone())
    }

    /// The implementation of System.Storage.Find.
    /// Finds the entries from the storage.
    fn find(&self, context: &StorageContext, prefix: &[u8], options: FindOptions) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        // Validate options
        if (options & !FindOptions::ALL) != FindOptions::empty() {
            panic!("Invalid FindOptions");
        }
        if options.contains(FindOptions::KEYS_ONLY) && 
           (options.contains(FindOptions::VALUES_ONLY) || 
            options.contains(FindOptions::DESERIALIZE_VALUES) || 
            options.contains(FindOptions::PICK_FIELD0) || 
            options.contains(FindOptions::PICK_FIELD1)) {
            panic!("Invalid FindOptions combination");
        }
        if options.contains(FindOptions::VALUES_ONLY) && 
           (options.contains(FindOptions::KEYS_ONLY) || 
            options.contains(FindOptions::REMOVE_PREFIX)) {
            panic!("Invalid FindOptions combination");
        }
        if options.contains(FindOptions::PICK_FIELD0) && options.contains(FindOptions::PICK_FIELD1) {
            panic!("Cannot pick both field 0 and field 1");
        }
        if (options.contains(FindOptions::PICK_FIELD0) || options.contains(FindOptions::PICK_FIELD1)) && 
           !options.contains(FindOptions::DESERIALIZE_VALUES) {
            panic!("PickField options require DeserializeValues");
        }

        let prefix_key = StorageKey::create_search_prefix(context.id, prefix);
        let direction = if options.contains(FindOptions::BACKWARDS) {
            SeekDirection::Backward
        } else {
            SeekDirection::Forward
        };

        let iterator = self.snapshot_cache.find(&prefix_key, direction);
        Box::new(StorageIterator::new(iterator, prefix.len(), options))
    }

    /// The implementation of System.Storage.Put.
    /// Puts a new entry into the storage.
    fn put(&mut self, context: &StorageContext, key: &[u8], value: &[u8]) -> Result<(), String> {
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key length too big".to_string());
        }
        if value.len() > MAX_STORAGE_VALUE_SIZE {
            return Err("Value length too big".to_string());
        }
        if context.is_read_only {
            return Err("StorageContext is readonly".to_string());
        }

        let storage_key = StorageKey {
            id: context.id,
            key: key.to_vec(),
        };

        let new_data_size: usize;
        let mut item = self.snapshot_cache.get_and_change(&storage_key);

        if item.is_none() {
            new_data_size = key.len() + value.len();
            item = Some(StorageItem::default());
            self.snapshot_cache.add(storage_key.clone(), item.as_ref().unwrap().clone());
        } else {
            let item = item.as_mut().unwrap();
            if value.is_empty() {
                new_data_size = 0;
            } else if value.len() <= item.value.len() {
                new_data_size = (value.len() - 1) / 4 + 1;
            } else if item.value.is_empty() {
                new_data_size = value.len();
            } else {
                new_data_size = (item.value.len() - 1) / 4 + 1 + value.len() - item.value.len();
            }
        }

        self.add_fee(new_data_size as u64 * self.storage_price);

        if let Some(item) = item {
            item.value = value.to_vec();
        }

        Ok(())
    }

    /// The implementation of System.Storage.Delete.
    /// Deletes an entry from the storage.
    fn delete(&mut self, context: &StorageContext, key: &[u8]) -> Result<(), String> {
        if context.is_read_only {
            return Err("StorageContext is readonly".to_string());
        }

        let storage_key = StorageKey {
            id: context.id,
            key: key.to_vec(),
        };

        self.snapshot_cache.delete(&storage_key);
        Ok(())
    }
}

// Helper functions and additional implementations...

impl ApplicationEngine {
    /// Validates the storage key size.
    fn validate_key_size(key: &[u8]) -> Result<(), String> {
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err(format!("Storage key exceeds maximum length of {} bytes", MAX_STORAGE_KEY_SIZE));
        }
        Ok(())
    }

    /// Validates the storage value size.
    fn validate_value_size(value: &[u8]) -> Result<(), String> {
        if value.len() > MAX_STORAGE_VALUE_SIZE {
            return Err(format!("Storage value exceeds maximum length of {} bytes", MAX_STORAGE_VALUE_SIZE));
        }
        Ok(())
    }

    /// Calculates the storage fee for a given operation.
    fn calculate_storage_fee(&self, old_size: usize, new_size: usize) -> u64 {
        let size_diff = if new_size > old_size {
            new_size - old_size
        } else {
            0
        };
        size_diff as u64 * self.storage_price
    }

    /// Checks if the contract has enough GAS to cover the storage fee.
    fn check_storage_fee(&self, fee: u64) -> Result<(), String> {
        if self.gas_left < fee {
            return Err("Insufficient GAS for storage operation".to_string());
        }
        Ok(())
    }
}

// Additional utility functions for storage operations
pub mod storage_utils {
    use super::*;

    /// Converts a StorageContext to a read-only version.
    pub fn make_read_only(context: StorageContext) -> StorageContext {
        StorageContext {
            id: context.id,
            is_read_only: true,
        }
    }

    /// Merges two storage keys.
    pub fn merge_keys(prefix: &[u8], key: &[u8]) -> Vec<u8> {
        [prefix, key].concat()
    }
}
