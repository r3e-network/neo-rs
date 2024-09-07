use neo::io::IVerifiable;
use neo::network::p2p::payloads::UInt160;
use neo::vm::types::{Array, ReferenceCounter, StackItem};
use neo::vm::ApplicationEngine;
use std::rc::Rc;

pub struct NotifyEventArgs {
    /// The container that containing the executed script.
    pub script_container: Rc<dyn IVerifiable>,

    /// The script hash of the contract that sends the log.
    pub script_hash: UInt160,

    /// The name of the event.
    pub event_name: String,

    /// The arguments of the event.
    pub state: Array,
}

impl NotifyEventArgs {
    /// Initializes a new instance of the NotifyEventArgs struct.
    pub fn new(container: Rc<dyn IVerifiable>, script_hash: UInt160, event_name: String, state: Array) -> Self {
        Self {
            script_container: container,
            script_hash,
            event_name,
            state,
        }
    }
}

impl IInteroperable for NotifyEventArgs {
    fn from_stack_item(&mut self, _stack_item: StackItem) {
        unimplemented!("FromStackItem is not supported for NotifyEventArgs");
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        Array::new_with_items(reference_counter, vec![
            StackItem::from(self.script_hash.to_vec()),
            StackItem::from(self.event_name.clone()),
            self.state.clone(),
        ])
    }

    fn to_stack_item_with_engine(&self, reference_counter: &mut ReferenceCounter, engine: &ApplicationEngine) -> StackItem {
        if engine.is_hardfork_enabled(Hardfork::HF_Domovoi) {
            Array::new_with_items(reference_counter, vec![
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
