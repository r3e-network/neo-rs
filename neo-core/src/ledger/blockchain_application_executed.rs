
use neo_core::network::p2p::payloads::Transaction;
use neo_core::smart_contract::TriggerType;
use neo_core::vm::{VMState, StackItem};
use neo_core::vm::types::NotifyEventArgs;
use std::error::Error;

pub mod ledger {
    use super::*;

    pub struct Blockchain;

    impl Blockchain {
        pub struct ApplicationExecuted {
            /// The transaction that contains the executed script. This field could be None if the contract is invoked by system.
            pub transaction: Option<Transaction>,

            /// The trigger of the execution.
            pub trigger: TriggerType,

            /// The state of the virtual machine after the contract is executed.
            pub vm_state: VMState,

            /// The error that caused the execution to terminate abnormally. This field could be None if the execution ends normally.
            pub error: Option<Box<dyn Error>>,

            /// GAS spent to execute.
            pub gas_consumed: i64,

            /// Items on the stack of the virtual machine after execution.
            pub stack: Vec<StackItem>,

            /// The notifications sent during the execution.
            pub notifications: Vec<NotifyEventArgs>,
        }

        impl ApplicationExecuted {
            pub fn new(engine: &ApplicationEngine) -> Self {
                Self {
                    transaction: engine.script_container().and_then(|container| container.downcast_ref::<Transaction>().cloned()),
                    trigger: engine.trigger(),
                    vm_state: engine.state(),
                    gas_consumed: engine.fee_consumed(),
                    error: engine.fault_exception().map(|e| Box::new(e) as Box<dyn Error>),
                    stack: engine.result_stack().to_vec(),
                    notifications: engine.notifications().to_vec(),
                }
            }
        }
    }
}
