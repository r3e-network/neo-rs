use std::collections::HashMap;
use crate::neo_contract::manifest::contract_parameter_definition::ContractParameterDefinition;

/// Represents an event in a smart contract ABI.
#[derive(Clone, Debug)]
pub struct ContractEventDescriptor {
    /// The name of the event or method.
    pub name: String,
    /// The parameters of the event or method.
    pub parameters: Vec<ContractParameterDefinition>,
}

impl IInteroperable for ContractEventDescriptor {
    fn from_stack_item(stack_item: &StackItem) -> Result<Self, Error> {
        if let StackItem::Struct(s) = stack_item {
            let name = s.get(0).ok_or(Error::InvalidFormat)?.as_string()?;
            let parameters = s.get(1).ok_or(Error::InvalidFormat)?
                .as_array()?
                .iter()
                .map(|p| ContractParameterDefinition::from_stack_item(p))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Self { name, parameters })
        } else {
            Err(Error::InvalidFormat)
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::Struct(Struct::new(vec![
            StackItem::String(self.name.clone()),
            StackItem::Array(Array::new(
                self.parameters
                    .iter()
                    .map(|p| p.to_stack_item())
                    .collect(),
            )),
        ]))
    }
}

impl ContractEventDescriptor {
    /// Converts the event from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The event represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted event.
    pub fn from_json(json: &Json) -> Result<Self, Error> {
        let name = json.get("name")
            .ok_or(Error::InvalidFormat)?
            .as_string()
            .ok_or(Error::InvalidFormat)?;
        
        if name.is_empty() {
            return Err(Error::InvalidFormat);
        }

        let parameters = json.get("parameters")
            .ok_or(Error::InvalidFormat)?
            .as_array()
            .ok_or(Error::InvalidFormat)?
            .iter()
            .map(|u| ContractParameterDefinition::from_json(u))
            .collect::<Result<Vec<_>, _>>()?;

        // Validate that parameter names are unique
        let mut param_names = HashMap::new();
        for param in &parameters {
            if param_names.insert(param.name.clone(), ()).is_some() {
                return Err(Error::InvalidFormat);
            }
        }

        Ok(Self { name, parameters })
    }

    /// Converts the event to a JSON object.
    ///
    /// # Returns
    ///
    /// The event represented by a JSON object.
    pub fn to_json(&self) -> Json {
        let mut json = Json::new_object();
        json.insert("name", Json::from(self.name.clone()));
        json.insert("parameters", Json::from(self.parameters.iter().map(|u| u.to_json()).collect::<Vec<_>>()));
        json
    }
}
