use crate::neo_vm::StackItem;
use crate::network::p2p::payloads::Transaction;
use crate::smart_contract::{ApplicationEngine, LogEventArgs, NotifyEventArgs, TriggerType};
use neo_vm::VmState as VMState;

/// Result of executing a script in the application engine, matching the C#
/// `Neo.SmartContract.ApplicationExecuted` notification.
#[derive(Clone)]
pub struct ApplicationExecuted {
    /// The transaction that triggered the execution, if any.
    pub transaction: Option<Transaction>,
    /// The trigger that caused the execution (e.g. application or verification).
    pub trigger: TriggerType,
    /// Final Neo VM state after execution (HALT, FAULT, etc.).
    pub vm_state: VMState,
    /// The fault exception message if execution failed.
    pub exception: Option<String>,
    /// Total GAS consumed by the execution.
    pub gas_consumed: i64,
    /// Result stack items remaining on the evaluation stack.
    pub stack: Vec<StackItem>,
    /// Notifications emitted via `System.Runtime.Notify` during execution.
    pub notifications: Vec<NotifyEventArgs>,
    /// Log messages emitted via `System.Runtime.Log` during execution.
    pub logs: Vec<LogEventArgs>,
}

impl ApplicationExecuted {
    /// Creates from an ApplicationEngine after execution.
    /// Reserved for block execution pipeline integration.
    #[allow(dead_code)]
    pub(crate) fn new(engine: &mut ApplicationEngine) -> Self {
        let transaction = engine.script_container().and_then(|c| {
            c.as_ref()
                .as_any()
                .downcast_ref::<crate::network::p2p::payloads::Transaction>()
                .cloned()
        });

        if let Some(tx) = transaction.as_ref() {
            let hash = tx.hash();
            let _ = engine.record_transaction_vm_state(&hash, engine.state());
        }

        Self {
            transaction,
            trigger: engine.trigger(),
            vm_state: engine.state(),
            gas_consumed: engine.fee_consumed(),
            exception: engine.fault_exception().map(|e| e.to_string()),
            stack: engine.result_stack().to_vec(),
            notifications: engine.notifications().to_vec(),
            logs: engine.logs().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UInt160;
    use crate::WitnessScope;
    use crate::ledger::{Block, BlockHeader};
    use crate::network::p2p::payloads::signer::Signer;
    use crate::network::p2p::payloads::witness::Witness;
    use crate::persistence::data_cache::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::smart_contract::TriggerType;
    use crate::smart_contract::application_engine::TEST_MODE_GAS;
    use crate::smart_contract::native::{LedgerContract, NativeContract};
    use std::sync::Arc;

    fn signed_transaction() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_valid_until_block(10);
        tx.add_signer(Signer::new(
            UInt160::default(),
            WitnessScope::CALLED_BY_ENTRY,
        ));
        tx.add_witness(Witness::new());
        tx
    }

    #[test]
    fn application_executed_records_vm_state_for_ledger_contract() {
        let mut transaction = signed_transaction();
        transaction.set_script(vec![0x01, 0x02, 0x03]);
        let transaction_hash = transaction.hash();

        let container: Arc<dyn crate::Verifiable> =
            Arc::new(transaction.clone()) as Arc<dyn crate::Verifiable>;
        let block = Block::new(BlockHeader::default(), vec![transaction.clone()]);
        let snapshot = Arc::new(DataCache::new(false));

        let mut engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            Some(container),
            Arc::clone(&snapshot),
            Some(block.clone()),
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
        .expect("failed to create engine");

        engine.force_vm_state(VMState::HALT);

        let ledger = LedgerContract::new();
        NativeContract::on_persist(&ledger, &mut engine).expect("on_persist");

        ApplicationExecuted::new(&mut engine);

        NativeContract::post_persist(&ledger, &mut engine).expect("post_persist");

        let stored_state = ledger
            .get_transaction_state(snapshot.as_ref(), &transaction_hash)
            .expect("state query")
            .expect("state present");

        assert_eq!(stored_state.vm_state(), VMState::HALT);
    }
}
