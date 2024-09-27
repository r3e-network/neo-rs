use neo_vm::stack_item::StackItem;
use neo_vm::References;
use neo_vm::stackitem_type::Struct;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::json::Json;
use std::fmt;

/// Represents a parameter of an event or method in ABI.
#[derive(Clone, Debug)]
pub struct ContractParameterDefinition {
    /// The name of the parameter.
    pub name: String,
    /// The type of the parameter. It can be any value of ContractParameterType except Void.
    pub parameter_type: ContractParameterType,
}

impl IInteroperable for ContractParameterDefinition {
    type Error = std::io::Error;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            if s.len() != 2 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid structure"));
            }
            Ok(Self {
                name: s[0].as_string()?,
                parameter_type: ContractParameterType::try_from(s[1].as_integer()? as u8)?,
            })
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid stack item type"))
        }
    }

    fn to_stack_item(&self, reference_counter: &mut References) -> Result<Rc<StackItem>, Self::Error> {
        Ok(Rc::new( StackItem::Struct(Struct::new(vec![
            StackItem::String(self.name.clone()),
            StackItem::Integer((self.parameter_type as u8).into()),
        ], reference_counter))))
    }
}

impl ContractParameterDefinition {
    /// Converts the parameter from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The parameter represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or if the parameter type is unsupported.
    pub fn from_json(json: &Json) -> Result<Self, fmt::Error> {
        let name = json["name"].as_str().ok_or(fmt::Error)?.to_string();
        if name.is_empty() {
            return Err(fmt::Error);
        }

        let type_str = json["type"].as_str().ok_or(fmt::Error)?;
        let parameter_type = ContractParameterType::from_str(type_str)
            .map_err(|_| fmt::Error)?;

        if parameter_type == ContractParameterType::Void {
            return Err(fmt::Error);
        }

        Ok(Self {
            name,
            parameter_type,
        })
    }

    /// Converts the parameter to a JSON object.
    ///
    /// # Returns
    ///
    /// The parameter represented by a JSON object.
    pub fn to_json(&self) -> Json {
        let mut json = Json::new_object();
        json.insert("name", Json::from(self.name.clone()));
        json.insert("type", Json::from(self.parameter_type.to_string()));
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_parameter_definition_serialization() {
        let param = ContractParameterDefinition {
            name: "test".to_string(),
            parameter_type: ContractParameterType::String,
        };

        let mut reference_counter = References::new();
        let stack_item = param.to_stack_item(&mut reference_counter).unwrap();
        let deserialized = ContractParameterDefinition::from_stack_item(&stack_item).unwrap();

        assert_eq!(param.name, deserialized.name);
        assert_eq!(param.parameter_type, deserialized.parameter_type);
    }

    #[test]
    fn test_contract_parameter_definition_json() {
        let mut json = Json::new_object();
        json.insert("name", Json::from("amount"));
        json.insert("type", Json::from("Integer"));

        let param = ContractParameterDefinition::from_json(&json).unwrap();
        assert_eq!(param.name, "amount");
        assert_eq!(param.parameter_type, ContractParameterType::Integer);

        let json_out = param.to_json();
        assert_eq!(json_out["name"].as_str().unwrap(), "amount");
        assert_eq!(json_out["type"].as_str().unwrap(), "Integer");
    }

    #[test]
    fn test_invalid_json() {
        let mut json = Json::new_object();
        json.insert("name", Json::from(""));
        json.insert("type", Json::from("Integer"));
        assert!(ContractParameterDefinition::from_json(&json).is_err());

        let mut json = Json::new_object();
        json.insert("name", Json::from("test"));
        json.insert("type", Json::from("Void"));
        assert!(ContractParameterDefinition::from_json(&json).is_err());
    }
}
