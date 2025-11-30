//! ValidatorAttribute - matches C# Neo.SmartContract.ValidatorAttribute exactly

use neo_vm::StackItem;

/// Abstract base for validator attributes (matches C# ValidatorAttribute)
/// Note: In C# this is an abstract class with [AttributeUsage(AttributeTargets.Parameter)]
/// In Rust, we implement this as a trait
pub trait ValidatorAttribute: std::fmt::Debug {
    /// Validates a stack item
    fn validate(&self, item: &StackItem) -> Result<(), String>;

    /// Clone the validator
    fn clone_box(&self) -> Box<dyn ValidatorAttribute>;
}

impl Clone for Box<dyn ValidatorAttribute> {
    fn clone(&self) -> Box<dyn ValidatorAttribute> {
        self.clone_box()
    }
}

// Example implementation for a max length validator
#[derive(Clone, Debug)]
pub struct MaxLengthValidator {
    pub max_length: usize,
}

impl MaxLengthValidator {
    pub fn new(max_length: usize) -> Self {
        Self { max_length }
    }
}

impl ValidatorAttribute for MaxLengthValidator {
    fn validate(&self, item: &StackItem) -> Result<(), String> {
        match item {
            StackItem::ByteString(bytes) if bytes.len() > self.max_length => Err(format!(
                "ByteString exceeds maximum length of {}",
                self.max_length
            )),
            StackItem::Buffer(buffer) if buffer.data().len() > self.max_length => Err(format!(
                "Buffer exceeds maximum length of {}",
                self.max_length
            )),
            _ => Ok(()),
        }
    }

    fn clone_box(&self) -> Box<dyn ValidatorAttribute> {
        Box::new(self.clone())
    }
}
