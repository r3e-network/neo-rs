use neo_vm::types::StackItem;

#[derive(Clone)]
pub trait ValidatorAttribute {
    fn validate(&self, item: &StackItem);
}

#[derive(Clone)]
pub struct ValidatorAttributeImpl;

impl ValidatorAttribute for ValidatorAttributeImpl {
    fn validate(&self, item: &StackItem) {
        // Implementation to be added
    }
}
