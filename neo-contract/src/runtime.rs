use alloc::{string::String, vec::Vec};
use core::mem;

use neo_base::{hash::Hash160, Bytes};
use neo_store::{ColumnId, Store};

use crate::{error::ContractError, manifest::ParameterKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Bytes(Bytes),
    String(String),
}

impl Value {
    pub fn kind(&self) -> ParameterKind {
        match self {
            Value::Null => ParameterKind::ByteArray,
            Value::Bool(_) => ParameterKind::Boolean,
            Value::Int(_) => ParameterKind::Integer,
            Value::Bytes(_) => ParameterKind::ByteArray,
            Value::String(_) => ParameterKind::String,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvocationResult {
    pub value: Value,
    pub gas_used: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GasMeter {
    limit: u64,
    consumed: u64,
}

impl GasMeter {
    pub fn new(limit: u64) -> Self {
        Self { limit, consumed: 0 }
    }

    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.consumed)
    }

    pub fn charge(&mut self, amount: u64) -> Result<(), ContractError> {
        self.consumed = self.consumed.saturating_add(amount);
        if self.consumed > self.limit {
            Err(ContractError::Runtime("out of gas"))
        } else {
            Ok(())
        }
    }

    pub fn consumed(&self) -> u64 {
        self.consumed
    }
}

pub struct ExecutionContext<'a> {
    store: &'a mut dyn Store,
    gas: GasMeter,
    signer: Option<Hash160>,
    log: Vec<String>,
    notifications: Vec<(String, Vec<Value>)>,
    timestamp: i64,
    invocation_counter: u32,
    storage_context: StorageContext,
    script: Bytes,
}

impl<'a> ExecutionContext<'a> {
    pub fn new(store: &'a mut dyn Store, gas_limit: u64, signer: Option<Hash160>) -> Self {
        Self {
            store,
            gas: GasMeter::new(gas_limit),
            signer,
            log: Vec::new(),
            notifications: Vec::new(),
            timestamp: 0,
            invocation_counter: 0,
            storage_context: StorageContext::default(),
            script: Bytes::default(),
        }
    }

    pub fn with_timestamp(
        store: &'a mut dyn Store,
        gas_limit: u64,
        signer: Option<Hash160>,
        timestamp: i64,
    ) -> Self {
        let mut ctx = Self::new(store, gas_limit, signer);
        ctx.timestamp = timestamp;
        ctx.invocation_counter = 0;
        ctx
    }

    pub fn gas_mut(&mut self) -> &mut GasMeter {
        &mut self.gas
    }

    pub fn gas(&self) -> &GasMeter {
        &self.gas
    }

    pub fn signer(&self) -> Option<Hash160> {
        self.signer
    }

    pub fn store(&self) -> &dyn Store {
        self.store
    }

    pub fn store_mut(&mut self) -> &mut dyn Store {
        self.store
    }

    pub fn load(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, ContractError> {
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
        self.store
            .put(column, key, value)
            .map_err(|_| ContractError::Storage("put failed"))
    }

    pub fn delete(&mut self, column: ColumnId, key: &[u8]) -> Result<(), ContractError> {
        self.store
            .delete(column, key)
            .map_err(|_| ContractError::Storage("delete failed"))
    }

    pub fn push_log(&mut self, message: String) {
        self.log.push(message);
    }

    pub fn logs(&self) -> &[String] {
        &self.log
    }

    pub fn push_notification(&mut self, name: String, payload: Vec<Value>) {
        self.notifications.push((name, payload));
    }

    pub fn notifications(&self) -> &[(String, Vec<Value>)] {
        &self.notifications
    }

    pub fn drain_logs(&mut self) -> Vec<String> {
        mem::take(&mut self.log)
    }

    pub fn drain_notifications(&mut self) -> Vec<(String, Vec<Value>)> {
        mem::take(&mut self.notifications)
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

    pub fn storage_context(&self) -> StorageContext {
        self.storage_context
    }

    pub fn set_storage_context(&mut self, context: StorageContext) {
        self.storage_context = context;
    }

    pub fn script(&self) -> &Bytes {
        &self.script
    }

    pub fn set_script(&mut self, script: Bytes) {
        self.script = script;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageContext {
    column: ColumnId,
}

impl StorageContext {
    pub const fn new(column: ColumnId) -> Self {
        Self { column }
    }

    pub const fn column(self) -> ColumnId {
        self.column
    }

    pub fn to_bytes(self) -> Bytes {
        Bytes::from(self.column.name().as_bytes().to_vec())
    }
}

impl Default for StorageContext {
    fn default() -> Self {
        StorageContext::new(ColumnId::new("contract"))
    }
}
