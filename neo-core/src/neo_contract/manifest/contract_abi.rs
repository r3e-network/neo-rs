use std::collections::HashMap;
use neo_vm::stack_item::StackItem;
use crate::neo_contract::manifest::contract_event_descriptor::ContractEventDescriptor;
use crate::neo_contract::manifest::contract_method_descriptor::ContractMethodDescriptor;

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
    pub fn from_json(json: &Json) -> Result<Self, Error> {
        let methods = json.get("methods")
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().filter_map(|u| ContractMethodDescriptor::from_json(u).ok()).collect())
            .unwrap_or_default();
        
        let events = json.get("events")
            .and_then(|e| e.as_array())
            .map(|arr| arr.iter().filter_map(|u| ContractEventDescriptor::from_json(u).ok()).collect())
            .unwrap_or_default();

        if methods.is_empty() {
            return Err(Error::Format);
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
    pub fn to_json(&self) -> Json {
        let mut json = Json::new_object();
        json.insert("methods", Json::from(self.methods.iter().map(|m| m.to_json()).collect::<Vec<_>>()));
        json.insert("events", Json::from(self.events.iter().map(|e| e.to_json()).collect::<Vec<_>>()));
        json
    }
}

impl IInteroperable for ContractAbi {
    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Error> {
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
            Err(Error::InvalidStackItemType)
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::Struct(Struct::new(vec![
            StackItem::Array(Array::new(self.methods.iter().map(|m| m.to_stack_item(reference_counter)).collect())),
            StackItem::Array(Array::new(self.events.iter().map(|e| e.to_stack_item(reference_counter)).collect())),
        ])))
    }
}
