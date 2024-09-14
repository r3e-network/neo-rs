use alloc::rc::Rc;
use std::collections::HashMap;
use neo_json::json_convert_trait::IJsonConvertible;
use neo_json::json_error::JsonError;
use neo_json::jtoken::JToken;
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::manifest::contract_parameter_definition::ContractParameterDefinition;
use crate::neo_contract::manifest::manifest_error::ManifestError;

/// Represents an event in a smart contract ABI.
#[derive(Clone, Debug)]
pub struct ContractEventDescriptor {
    /// The name of the event or method.
    pub name: String,
    /// The parameters of the event or method.
    pub parameters: Vec<ContractParameterDefinition>,
}

impl Default for ContractEventDescriptor {
    fn default() -> Self {
        todo!()
    }
}

impl IInteroperable for ContractEventDescriptor {
    type Error = ManifestError;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            let name = s.get(0).ok_or(Self::Error::InvalidFormat)?.as_string()?;
            let parameters = s.get(1).ok_or(Self::Error::InvalidFormat)?
                .as_array()?
                .iter()
                .map(|p| ContractParameterDefinition::from_stack_item(p))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Self { name, parameters })
        } else {
            Err(Self::Error::InvalidFormat)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        Ok(Rc::from(StackItem::Struct(vec![
            Rc::from(self.name.clone()),
            Rc::from(StackItem::Array(
                self.parameters
                    .iter()
                    .map(|p| Rc::from(p.to_stack_item()))
                    .collect(),
            )),
        ])))
    }

}

impl IJsonConvertible for ContractEventDescriptor {
    /// Converts the event to a JSON object.
    ///
    /// # Returns
    ///
    /// The event represented by a JSON object.
     fn to_json(&self) -> JToken {
        let mut json = JToken::new_object();
        json.insert("name".to_string(), JToken::from(self.name.clone()));
        json.insert("parameters".to_string(), JToken::from(self.parameters.iter().map(|u| u.to_json()).collect::<Vec<_>>()));
        json
    }

    /// Converts the event from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The event represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted event.
     fn from_json(json: &JToken) -> Result<Self, JsonError> {
        let name = json.get("name")
            .ok_or(JsonError::InvalidFormat)?
            .as_string()
            .ok_or(JsonError::InvalidFormat)?;

        if name.is_empty() {
            return Err(JsonError::InvalidFormat);
        }

        let parameters = json.get("parameters")
            .ok_or(JsonError::InvalidFormat)?
            .as_array()
            .ok_or(JsonError::InvalidFormat)?
            .iter()
            .map(|u| ContractParameterDefinition::from_json(u))
            .collect::<Result<Vec<_>, _>>()?;

        // Validate that parameter names are unique
        let mut param_names = HashMap::new();
        for param in &parameters {
            if param_names.insert(param.name.clone(), ()).is_some() {
                return Err(JsonError::InvalidFormat);
            }
        }

        Ok(Self { name, parameters })
    }
}
