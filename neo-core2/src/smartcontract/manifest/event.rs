use crate::vm::stackitem::{self, Item};
use crate::smartcontract::manifest::Parameter;
use crate::smartcontract::ParameterType;
use std::error::Error;

/// Event is a description of a single event.
#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub parameters: Vec<Parameter>,
}

impl Event {
    /// Checks Event consistency and correctness.
    pub fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        if self.name.is_empty() {
            return Err("empty or absent name".into());
        }
        Parameter::are_valid(&self.parameters)
    }

    /// Converts Event to stackitem::Item.
    pub fn to_stack_item(&self) -> Item {
        let params: Vec<Item> = self.parameters.iter().map(Parameter::to_stack_item).collect();
        stackitem::Struct::new(vec![
            stackitem::ByteArray::from(self.name.as_bytes()).into(),
            stackitem::Array::new(params).into(),
        ]).into()
    }

    /// Converts stackitem::Item to Event.
    pub fn from_stack_item(item: &Item) -> Result<Self, Box<dyn Error>> {
        let event = item.as_struct().ok_or("invalid Event stackitem type")?;
        let items = event.value();

        if items.len() != 2 {
            return Err("invalid Event stackitem length".into());
        }

        let name = String::from_utf8(items[0].as_byte_array()?.to_vec())?;
        let params = items[1].as_array().ok_or("invalid Params stackitem type")?;

        let parameters = params.iter()
            .map(Parameter::from_stack_item)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Event { name, parameters })
    }

    /// Checks compliance of the given array of items with the current event.
    pub fn check_compliance(&self, items: &[Item]) -> Result<(), Box<dyn Error>> {
        if items.len() != self.parameters.len() {
            return Err(format!("mismatch between the number of parameters and items: {} vs {}", 
                self.parameters.len(), items.len()).into());
        }

        for (i, (param, item)) in self.parameters.iter().zip(items.iter()).enumerate() {
            if !param.param_type.matches(item) {
                return Err(format!("parameter {} type mismatch: {} (manifest) vs {} (notification)", 
                    i, param.param_type, item.type_()).into());
            }
        }

        Ok(())
    }
}
