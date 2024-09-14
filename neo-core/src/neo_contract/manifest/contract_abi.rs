use alloc::rc::Rc;
use std::collections::HashMap;
use neo_json::json_convert_trait::IJsonConvertible;
use neo_json::jtoken::JToken;
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::manifest::contract_event_descriptor::ContractEventDescriptor;
use crate::neo_contract::manifest::contract_method_descriptor::ContractMethodDescriptor;
use crate::neo_contract::manifest::manifest_error::ManifestError;

/// Represents the ABI of a smart contract.
///
/// For more details, see NEP-14.
#[derive(Clone, Debug)]
pub struct ContractAbi {
    pub(crate) methods: Vec<ContractMethodDescriptor>,
    pub(crate) events: Vec<ContractEventDescriptor>,
    method_dictionary: Option<HashMap<(String, usize), ContractMethodDescriptor>>,
}

impl ContractAbi {
    /// Creates a new ContractAbi from JSON.
    pub fn from_json(json: &JToken) -> Result<Self, ManifestError> {
        let methods = json.get("methods")
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().filter_map(|u| ContractMethodDescriptor::from_json(u).ok()).collect())
            .unwrap_or_default();
        
        let events = json.get("events")
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|u| ContractEventDescriptor::from_json(u).ok()).collect())
            .unwrap_or_default();

        if methods.is_empty() {
            return Err(ManifestError::InvalidFormat("".to_string()));
        }

        Ok(Self {
            methods,
            events,
            method_dictionary: None,
        })
    }

    /// Gets the method with the specified name and parameter count.
    pub fn get_method(&mut self, name: &str, pcount: i32) -> Option<&ContractMethodDescriptor> {
        if pcount < -1 || pcount > u16::MAX as i32 {
            return None;
        }

        if pcount >= 0 {
            if self.method_dictionary.is_none() {
                let dict = self.methods.iter().map(|m| {
                    ((m.name().to_string(), m.parameters().len()), m.clone())
                }).collect();
                self.method_dictionary = Some(dict);
            }
            self.method_dictionary.as_ref().unwrap().get(&(name.to_string(), pcount as usize))
        } else {
            self.methods.iter().find(|m| m.name() == name)
        }
    }

    /// Converts the ABI to a JSON object.
    pub fn to_json(&self) -> JToken {
        let mut json = JToken::new_object();
        json.insert("methods".to_string(), JToken::from(self.methods.iter().map(|m| m.to_json()).collect::<Vec<_>>())).expect("TODO: panic message");
        json.insert("events".to_string(), JToken::from(self.events.iter().map(|e| e.to_json()).collect::<Vec<_>>())).expect("TODO: panic message");
        json
    }
}

impl Default for ContractAbi {
    fn default() -> Self {
        todo!()
    }
}

impl IInteroperable for ContractAbi {
    type Error = ManifestError;

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        if let StackItem::Struct(s) = stack_item {
            let methods = s.get(0)
                .and_then(|arr| arr.as_array())
                .map(|arr| arr.iter().filter_map(|item| ContractMethodDescriptor::from_stack_item(item.clone()).ok()).collect())
                .unwrap_or_default();

            let events = s.get(1)
                .and_then(|arr| arr.as_array())
                .map(|arr| arr.iter().filter_map(|item| ContractEventDescriptor::from_stack_item(item.clone()).ok()).collect())
                .unwrap_or_default();

            Ok(Self {
                methods,
                events,
                method_dictionary: None,
            })
        } else {
            Err(ManifestError::InvalidStackItemType)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        Ok(Rc::from(StackItem::Struct(vec![
            Rc::from(StackItem::Array(self.methods.iter().map(|m| Rc::new(m.to_stack_item(reference_counter)?)).collect::<Vec<_>>())),
            Rc::from(StackItem::Array(self.events.iter().map(|e| Rc::new(e.to_stack_item(reference_counter)?)).collect::<Vec<_>>())),
        ])))
    }
}
