use neo_crypto::sha256;
use std::collections::HashMap;
use std::sync::Once;

/// Represents a descriptor of an interoperable service.
#[derive(Clone)]
pub struct InteropDescriptor {
    /// The name of the interoperable service.
    pub name: String,

    hash: Once<u32>,

    /// The handler function for the interoperable service.
    pub handler: fn(),

    parameters: Once<Vec<InteropParameterDescriptor>>,

    /// The fixed price for calling the interoperable service. It can be 0 if the interoperable service has a variable price.
    pub fixed_price: i64,

    /// The required CallFlags for the interoperable service.
    pub required_call_flags: CallFlags,
}

impl InteropDescriptor {
    /// The hash of the interoperable service.
    pub fn hash(&self) -> u32 {
        self.hash.call_once(|| {
            let bytes = self.name.as_bytes();
            let hash = sha256(bytes);
            u32::from_le_bytes(hash[0..4].try_into().unwrap())
        })
    }

    /// The parameters of the interoperable service.
    pub fn parameters(&self) -> &[InteropParameterDescriptor] {
        self.parameters.call_once(|| {
            // Note: This is a placeholder. In Rust, we'd need to implement
            // a way to get parameter information from the handler function.
            Vec::new()
        });
        self.parameters.get().unwrap()
    }
}

impl From<&InteropDescriptor> for u32 {
    fn from(descriptor: &InteropDescriptor) -> Self {
        descriptor.hash()
    }
}

// Note: InteropParameterDescriptor and CallFlags are not defined here.
// They would need to be implemented separately.
