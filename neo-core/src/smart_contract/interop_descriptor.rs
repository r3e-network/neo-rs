//! InteropDescriptor - matches C# Neo.SmartContract.InteropDescriptor exactly

use crate::hardfork::Hardfork;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::interop_parameter_descriptor::InteropParameterDescriptor;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// Represents a descriptor of an interoperable service (matches C# InteropDescriptor)
#[derive(Clone, Debug)]
pub struct InteropDescriptor {
    /// The name of the interoperable service
    pub name: String,

    /// Cached hash value
    hash_cache: OnceLock<u32>,

    /// Handler function pointer (simplified from C# MethodInfo)
    pub handler: fn(&mut crate::smart_contract::ApplicationEngine) -> Result<(), String>,

    /// The parameters of the interoperable service
    pub parameters: Vec<InteropParameterDescriptor>,

    /// The fixed price for calling the interoperable service
    pub fixed_price: i64,

    /// Required Hardfork to be active (if any)
    pub hardfork: Option<Hardfork>,

    /// The required CallFlags for the interoperable service
    pub required_call_flags: CallFlags,
}

impl InteropDescriptor {
    /// Creates a new InteropDescriptor
    pub fn new(
        name: String,
        handler: fn(&mut crate::smart_contract::ApplicationEngine) -> Result<(), String>,
        parameters: Vec<InteropParameterDescriptor>,
        fixed_price: i64,
        required_call_flags: CallFlags,
    ) -> Self {
        Self {
            name,
            hash_cache: OnceLock::new(),
            handler,
            parameters,
            fixed_price,
            hardfork: None,
            required_call_flags,
        }
    }

    /// Creates with hardfork requirement
    pub fn new_with_hardfork(
        name: String,
        handler: fn(&mut crate::smart_contract::ApplicationEngine) -> Result<(), String>,
        parameters: Vec<InteropParameterDescriptor>,
        fixed_price: i64,
        required_call_flags: CallFlags,
        hardfork: Hardfork,
    ) -> Self {
        Self {
            name,
            hash_cache: OnceLock::new(),
            handler,
            parameters,
            fixed_price,
            hardfork: Some(hardfork),
            required_call_flags,
        }
    }

    /// Gets the hash of the interoperable service
    pub fn hash(&self) -> u32 {
        *self.hash_cache.get_or_init(|| {
            // Compute SHA256 of ASCII bytes and take first 4 bytes as little-endian u32
            let mut hasher = Sha256::new();
            hasher.update(self.name.as_bytes());
            let result = hasher.finalize();
            u32::from_le_bytes([result[0], result[1], result[2], result[3]])
        })
    }

    /// Checks if this descriptor matches a given hash
    pub fn matches_hash(&self, hash: u32) -> bool {
        self.hash() == hash
    }

    /// Invokes the handler
    pub fn invoke(
        &self,
        engine: &mut crate::smart_contract::ApplicationEngine,
    ) -> Result<(), String> {
        // Check call flags
        if !engine.has_call_flags(self.required_call_flags) {
            return Err(format!(
                "Missing required call flags: {:?}",
                self.required_call_flags
            ));
        }

        // Check hardfork if required
        if let Some(ref hardfork) = self.hardfork {
            if !engine.is_hardfork_enabled(*hardfork) {
                return Err(format!("Hardfork {:?} not enabled", hardfork));
            }
        }

        // Invoke the handler
        (self.handler)(engine)
    }
}
