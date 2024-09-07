// Copyright (C) 2015-2024 The Neo Project.
//
// contract_parameter_definition.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.


use crate::neo_contract::contract_parameter_type::ContractParameterType;

/// Represents a parameter of an event or method in ABI.
#[derive(Clone, Debug)]
pub struct ContractParameterDefinition {
    /// The name of the parameter.
    pub name: String,
    /// The type of the parameter. It can be any value of ContractParameterType except Void.
    pub parameter_type: ContractParameterType,
}

impl Serializable for ContractParameterDefinition {
    fn from_stack_item(stack_item: &StackItem) -> Result<Self, Error> {
        if let StackItem::Struct(s) = stack_item {
            if s.len() != 2 {
                return Err(Error::InvalidStructure);
            }
            Ok(Self {
                name: s[0].as_string()?,
                parameter_type: ContractParameterType::try_from(s[1].as_integer()? as u8)?,
            })
        } else {
            Err(Error::InvalidStackItemType)
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::Struct(vec![
            StackItem::String(self.name.clone()),
            StackItem::Integer((self.parameter_type as u8).into()),
        ])
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
    pub fn from_json(json: &JsonValue) -> Result<Self, Error> {
        let name = json["name"].as_str().ok_or(Error::InvalidFormat)?.to_string();
        if name.is_empty() {
            return Err(Error::InvalidFormat);
        }

        let type_str = json["type"].as_str().ok_or(Error::InvalidFormat)?;
        let parameter_type = ContractParameterType::from_str(type_str)
            .map_err(|_| Error::InvalidFormat)?;

        if parameter_type == ContractParameterType::Void {
            return Err(Error::InvalidFormat);
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
    pub fn to_json(&self) -> JsonValue {
        json!({
            "name": self.name,
            "type": self.parameter_type.to_string()
        })
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

        let stack_item = param.to_stack_item();
        let deserialized = ContractParameterDefinition::from_stack_item(&stack_item).unwrap();

        assert_eq!(param.name, deserialized.name);
        assert_eq!(param.parameter_type, deserialized.parameter_type);
    }

    #[test]
    fn test_contract_parameter_definition_json() {
        let json = json!({
            "name": "amount",
            "type": "Integer"
        });

        let param = ContractParameterDefinition::from_json(&json).unwrap();
        assert_eq!(param.name, "amount");
        assert_eq!(param.parameter_type, ContractParameterType::Integer);

        let json_out = param.to_json();
        assert_eq!(json_out["name"], "amount");
        assert_eq!(json_out["type"], "Integer");
    }

    #[test]
    fn test_invalid_json() {
        let json = json!({
            "name": "",
            "type": "Integer"
        });
        assert!(ContractParameterDefinition::from_json(&json).is_err());

        let json = json!({
            "name": "test",
            "type": "Void"
        });
        assert!(ContractParameterDefinition::from_json(&json).is_err());
    }
}
