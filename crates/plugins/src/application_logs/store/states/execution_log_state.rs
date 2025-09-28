// Copyright (C) 2015-2025 The Neo Project.
//
// execution_log_state.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_vm::VMState;
use neo_core::ApplicationExecuted;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Execution log state matching C# ExecutionLogState exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLogState {
    /// VM state
    pub vm_state: VMState,
    /// Exception message
    pub exception: Option<String>,
    /// Gas consumed
    pub gas_consumed: i64,
    /// Stack item IDs
    pub stack_item_ids: Vec<String>,
}

impl ExecutionLogState {
    /// Creates a new ExecutionLogState
    /// Matches C# Create method
    pub fn create(app_execution: &ApplicationExecuted, stack_item_ids: Vec<String>) -> Self {
        Self {
            vm_state: app_execution.vm_state,
            exception: app_execution.exception.as_ref().map(|e| {
                e.inner_exception()
                    .map(|ie| ie.message().to_string())
                    .unwrap_or_else(|| e.message().to_string())
            }),
            gas_consumed: app_execution.gas_consumed,
            stack_item_ids,
        }
    }
    
    /// Gets the size of the serialized data
    /// Matches C# Size property
    pub fn size(&self) -> usize {
        1 + // VMState (byte)
        self.exception.as_ref().map(|e| e.len() + 4).unwrap_or(4) + // Exception (var string)
        8 + // GasConsumed (long)
        4 + // StackItemIds length (uint)
        self.stack_item_ids.iter().map(|id| id.len() + 4).sum::<usize>() // StackItemIds (var bytes)
    }
    
    /// Deserializes from bytes
    /// Matches C# Deserialize method
    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < 13 {
            return Err("Invalid data length".to_string());
        }
        
        let mut offset = 0;
        let vm_state = VMState::from_byte(data[offset])?;
        offset += 1;
        
        let (exception, exception_len) = Self::read_var_string(&data[offset..])?;
        offset += exception_len;
        
        if data.len() < offset + 8 {
            return Err("Invalid data length for gas consumed".to_string());
        }
        let gas_consumed = i64::from_le_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ]);
        offset += 8;
        
        if data.len() < offset + 4 {
            return Err("Invalid data length for stack item count".to_string());
        }
        let stack_item_count = u32::from_le_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
        ]) as usize;
        offset += 4;
        
        let mut stack_item_ids = Vec::new();
        for _ in 0..stack_item_count {
            let (id, id_len) = Self::read_var_bytes(&data[offset..])?;
            stack_item_ids.push(String::from_utf8(id).map_err(|_| "Invalid UTF-8 in stack item ID")?);
            offset += id_len;
        }
        
        Ok(Self {
            vm_state,
            exception: if exception.is_empty() { None } else { Some(exception) },
            gas_consumed,
            stack_item_ids,
        })
    }
    
    /// Serializes to bytes
    /// Matches C# Serialize method
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // VMState
        data.push(self.vm_state.to_byte());
        
        // Exception
        let exception = self.exception.as_deref().unwrap_or("");
        data.extend_from_slice(&self.write_var_string(exception));
        
        // GasConsumed
        data.extend_from_slice(&self.gas_consumed.to_le_bytes());
        
        // StackItemIds
        data.extend_from_slice(&(self.stack_item_ids.len() as u32).to_le_bytes());
        for id in &self.stack_item_ids {
            data.extend_from_slice(&self.write_var_bytes(id.as_bytes()));
        }
        
        data
    }
    
    // Helper methods
    
    fn read_var_string(data: &[u8]) -> Result<(String, usize), String> {
        if data.len() < 4 {
            return Err("Invalid data length for var string length".to_string());
        }
        
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let total_len = 4 + len;
        
        if data.len() < total_len {
            return Err("Invalid data length for var string".to_string());
        }
        
        let string = String::from_utf8(data[4..total_len].to_vec())
            .map_err(|_| "Invalid UTF-8 in var string")?;
        
        Ok((string, total_len))
    }
    
    fn read_var_bytes(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
        if data.len() < 4 {
            return Err("Invalid data length for var bytes length".to_string());
        }
        
        let len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let total_len = 4 + len;
        
        if data.len() < total_len {
            return Err("Invalid data length for var bytes".to_string());
        }
        
        let bytes = data[4..total_len].to_vec();
        Ok((bytes, total_len))
    }
    
    fn write_var_string(&self, s: &str) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(s.len() as u32).to_le_bytes());
        data.extend_from_slice(s.as_bytes());
        data
    }
    
    fn write_var_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(bytes);
        data
    }
}

impl PartialEq for ExecutionLogState {
    fn eq(&self, other: &Self) -> bool {
        self.vm_state == other.vm_state &&
        self.exception == other.exception &&
        self.gas_consumed == other.gas_consumed &&
        self.stack_item_ids == other.stack_item_ids
    }
}

impl Eq for ExecutionLogState {}

impl Hash for ExecutionLogState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.vm_state.hash(state);
        self.exception.hash(state);
        self.gas_consumed.hash(state);
        for id in &self.stack_item_ids {
            id.hash(state);
        }
    }
}

impl Default for ExecutionLogState {
    fn default() -> Self {
        Self {
            vm_state: VMState::NONE,
            exception: None,
            gas_consumed: 0,
            stack_item_ids: Vec::new(),
        }
    }
}