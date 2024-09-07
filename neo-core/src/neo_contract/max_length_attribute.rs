use neo_vm::types::StackItem;

pub struct MaxLengthAttribute {
    pub max_length: usize,
}

impl MaxLengthAttribute {
    pub fn new(max_length: usize) -> Self {
        MaxLengthAttribute { max_length }
    }

    pub fn validate(&self, item: &StackItem) -> Result<(), String> {
        if item.get_span().len() > self.max_length {
            Err("The input exceeds the maximum length.".to_string())
        } else {
            Ok(())
        }
    }
}
