//! ContractMethodMetadata - matches C# Neo.SmartContract.Native.ContractMethodMetadata exactly

use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;

/// Metadata for a native contract method (matches C# ContractMethodMetadata)
#[derive(Clone)]
pub struct ContractMethodMetadata {
    /// The name of the method
    pub name: String,
    
    /// The handler function
    pub handler: fn(&mut ApplicationEngine) -> Result<(), String>,
    
    /// The required call flags
    pub required_call_flags: CallFlags,
    
    /// The gas cost
    pub cpu_fee: i64,
    
    /// The storage fee
    pub storage_fee: i64,
}

impl ContractMethodMetadata {
    /// Creates new method metadata
    pub fn new(
        name: String,
        handler: fn(&mut ApplicationEngine) -> Result<(), String>,
        required_call_flags: CallFlags,
        cpu_fee: i64,
        storage_fee: i64,
    ) -> Self {
        Self {
            name,
            handler,
            required_call_flags,
            cpu_fee,
            storage_fee,
        }
    }
    
    /// Creates metadata for a safe read-only method
    pub fn read_only(
        name: String,
        handler: fn(&mut ApplicationEngine) -> Result<(), String>,
        cpu_fee: i64,
    ) -> Self {
        Self::new(name, handler, CallFlags::READ_STATES, cpu_fee, 0)
    }
    
    /// Creates metadata for a write method
    pub fn write(
        name: String,
        handler: fn(&mut ApplicationEngine) -> Result<(), String>,
        cpu_fee: i64,
        storage_fee: i64,
    ) -> Self {
        Self::new(name, handler, CallFlags::STATES, cpu_fee, storage_fee)
    }
    
    /// Invokes the method
    pub fn invoke(&self, engine: &mut ApplicationEngine) -> Result<(), String> {
        // Check call flags
        if !engine.has_call_flags(self.required_call_flags) {
            return Err(format!("Method {} requires call flags {:?}", self.name, self.required_call_flags));
        }
        
        // Add gas cost
        engine.add_gas(self.cpu_fee)?;
        
        // Add storage fee if applicable
        if self.storage_fee > 0 {
            engine.add_gas(self.storage_fee)?;
        }
        
        // Invoke handler
        (self.handler)(engine)
    }
}
