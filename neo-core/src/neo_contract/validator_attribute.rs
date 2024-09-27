use core::fmt::{Debug, Formatter};
use neo_vm::StackItem;

pub trait ValidatorTrait:Clone+Debug {
    fn validate(&self, item: &StackItem);
}

#[derive(Clone)]
pub struct ValidatorAttributeImpl;

impl Debug for ValidatorAttributeImpl {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        todo!()
    }
}

impl ValidatorTrait for ValidatorAttributeImpl {
    fn validate(&self, item: &StackItem) {
        // Implementation to be added
    }
}
