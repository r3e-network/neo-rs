use crate::smartcontract::{self, ParamType};
use crate::vm::stackitem::{self, Item, StackItem};
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

/// Parameter represents smartcontract's parameter's definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub param_type: ParamType,
}

/// Parameters is just a vector of Parameter.
pub type Parameters = Vec<Parameter>;

impl Parameter {
    /// Creates a new parameter with the specified name and type.
    pub fn new(name: String, param_type: ParamType) -> Self {
        Self { name, param_type }
    }

    /// Checks Parameter consistency and correctness.
    pub fn is_valid(&self) -> Result<(), Box<dyn Error>> {
        if self.name.is_empty() {
            return Err("empty or absent name".into());
        }
        if self.param_type == ParamType::Void {
            return Err("void parameter".into());
        }
        smartcontract::convert_to_param_type(self.param_type as i32)?;
        Ok(())
    }

    /// Converts Parameter to stackitem::Item.
    pub fn to_stack_item(&self) -> Box<dyn Item> {
        stackitem::Struct::new(vec![
            Box::new(stackitem::String::new(self.name.clone())),
            Box::new(stackitem::Integer::new(self.param_type as i32)),
        ])
    }

    /// Converts stackitem::Item to Parameter.
    pub fn from_stack_item(item: &dyn Item) -> Result<Self, Box<dyn Error>> {
        if item.r#type() != stackitem::Type::Struct {
            return Err("invalid Parameter stackitem type".into());
        }
        let param = item.value().as_slice();
        if param.len() != 2 {
            return Err("invalid Parameter stackitem length".into());
        }
        let name = param[0].to_string()?;
        let typ = param[1].try_integer()?;
        let param_type = smartcontract::convert_to_param_type(typ.to_i32()?)?;
        Ok(Self { name, param_type })
    }
}

impl Parameters {
    /// Checks all parameters for validity and consistency.
    pub fn are_valid(&self) -> Result<(), Box<dyn Error>> {
        for (i, param) in self.iter().enumerate() {
            param.is_valid().map_err(|e| {
                format!("parameter #{}/{}: {}", i, param.name, e).into()
            })?;
        }
        if slice_has_dups(self, |a, b| a.name.cmp(&b.name)) {
            return Err("duplicate parameter name".into());
        }
        Ok(())
    }
}

/// Checks the slice for duplicate elements.
fn slice_has_dups<T, F>(slice: &[T], cmp: F) -> bool
where
    F: Fn(&T, &T) -> Ordering,
{
    if slice.len() < 2 {
        return false;
    }
    let mut sorted = slice.to_vec();
    sorted.sort_by(cmp);
    sorted.windows(2).any(|w| cmp(&w[0], &w[1]) == Ordering::Equal)
}
