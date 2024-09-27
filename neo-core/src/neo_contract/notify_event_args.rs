use std::cell::RefCell;
use std::rc::Rc;
use neo_vm::References;
use neo_vm::StackItem;
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::contract_error::ContractError;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::network::payloads::IVerifiable;
use neo_type::H160;

#[derive(Default)]
pub struct NotifyEventArgs {
    /// The container that containing the executed script.
    pub script_container: Rc<dyn IVerifiable<Error=ContractError>>,

    /// The script hash of the contract that sends the log.
    pub script_hash: H160,

    /// The name of the event.
    pub event_name: String,

    /// The arguments of the event.
    /// StackItem::Array
    pub state: StackItem,
}

impl NotifyEventArgs {
    /// Initializes a new instance of the NotifyEventArgs struct.
    pub fn new(container: Rc<dyn IVerifiable<Error=ContractError>>, script_hash: H160, event_name: String, state: StackItem/*Array*/) -> Self {
        Self {
            script_container: container,
            script_hash,
            event_name,
            state,
        }
    }

    
    fn to_stack_item_with_engine(&self, reference_counter: Rc<RefCell<References>>, engine: &ApplicationEngine) -> StackItem {
        if engine.is_hardfork_enabled(Hardfork::HF_Domovoi) {
            StackItem::new_array(reference_counter, vec![
                StackItem::from(self.script_hash.to_vec()),
                StackItem::from(self.event_name.clone()),
                if self.state.on_stack() {
                    self.state.clone()
                } else {
                    self.state.deep_copy(true)
                },
            ])
        } else {
            self.to_stack_item(reference_counter)
        }
    }
    
}

impl IInteroperable for NotifyEventArgs {

    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error> {
        unimplemented!("FromStackItem is not supported for NotifyEventArgs");
    }

    fn to_stack_item(&self, reference_counter: Rc<RefCell< References>>) -> Result<Rc<StackItem>, Self::Error> {
        Ok(StackItem::new_array(reference_counter, vec![
            StackItem::from(self.script_hash.to_vec()),
            StackItem::from(self.event_name.clone()),
            self.state.clone(),
        ]))
    }
    
    type Error = std::io::Error;
}
