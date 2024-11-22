use std::cell::RefCell;
use once_cell::sync::OnceCell;
use crate::neo_contract::call_flags::CallFlags;
use crate::neo_contract::interop_parameter_descriptor::InteropParameterDescriptor;

/// Represents a descriptor of an interoperable service.
pub struct InteropDescriptor {
    /// The name of the interoperable service.
    pub name: String,

    hash: RefCell<OnceCell<u32>>,

    /// The handler function for the interoperable service.
    pub handler: fn(),

    parameters: RefCell<OnceCell<Vec<InteropParameterDescriptor>>>,

    /// The fixed price for calling the interoperable service. It can be 0 if the interoperable service has a variable price.
    pub fixed_price: i64,

    /// The required CallFlags for the interoperable service.
    pub required_call_flags: CallFlags,
}

impl InteropDescriptor {
    /// The hash of the interoperable service.
    pub fn hash(&self) -> u32 {
        *self.hash.borrow().get_or_init(|| {
            // Compute hash here
            0 // Placeholder
        })
    }

    /// The parameters of the interoperable service.
    pub fn parameters(&self) -> Vec<InteropParameterDescriptor> {
        self.parameters.borrow().get_or_init(|| {
            // Initialize parameters here
            Vec::new() // Placeholder
        }).to_vec()
    }
}

impl Clone for InteropDescriptor {
    fn clone(&self) -> Self {
        InteropDescriptor {
            name: self.name.clone(),
            hash: RefCell::new(OnceCell::new()),
            handler: self.handler,
            parameters: RefCell::new(OnceCell::new()),
            fixed_price: self.fixed_price,
            required_call_flags: self.required_call_flags,
        }
    }
}
