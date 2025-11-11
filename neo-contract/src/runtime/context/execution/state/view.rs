use alloc::string::String;

use neo_store::Store;
use neo_vm::Trigger;

use super::ExecutionContext;
use crate::runtime::{gas::GasMeter, storage::StorageContext};

impl<'a> ExecutionContext<'a> {
    pub fn gas_mut(&mut self) -> &mut GasMeter {
        &mut self.gas
    }

    pub fn gas(&self) -> &GasMeter {
        &self.gas
    }

    pub fn set_trigger(&mut self, trigger: Trigger) {
        self.trigger = trigger;
    }

    pub fn trigger(&self) -> Trigger {
        self.trigger
    }

    pub fn set_platform(&mut self, platform: impl Into<String>) {
        self.platform = platform.into();
    }

    pub fn platform(&self) -> &str {
        &self.platform
    }

    pub fn store(&self) -> &dyn Store {
        self.store
    }

    pub fn store_mut(&mut self) -> &mut dyn Store {
        self.store
    }

    pub fn storage_context(&self) -> StorageContext {
        self.storage_context
    }

    pub fn set_storage_context(&mut self, context: StorageContext) {
        self.storage_context = context;
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    pub fn set_timestamp(&mut self, timestamp: i64) {
        self.timestamp = timestamp;
    }

    pub fn invocation_counter(&self) -> u32 {
        self.invocation_counter
    }

    pub fn set_invocation_counter(&mut self, counter: u32) {
        self.invocation_counter = counter;
    }
}
