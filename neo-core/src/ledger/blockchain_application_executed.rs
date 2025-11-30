use crate::neo_vm::{StackItem, VMState};
use crate::network::p2p::payloads::Transaction;
use crate::smart_contract::{ApplicationEngine, NotifyEventArgs, TriggerType};

#[derive(Clone)]
pub struct ApplicationExecuted {
    pub transaction: Option<Transaction>,
    pub trigger: TriggerType,
    pub vm_state: VMState,
    pub exception: Option<String>,
    pub gas_consumed: i64,
    pub stack: Vec<StackItem>,
    pub notifications: Vec<NotifyEventArgs>,
}

impl ApplicationExecuted {
    pub(crate) fn new(engine: &mut ApplicationEngine) -> Self {
        let transaction = engine
            .script_container()
            .and_then(|c| c.as_ref().as_transaction().cloned());

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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Block, BlockHeader};
    use crate::network::p2p::payloads::signer::Signer;
    use crate::network::p2p::payloads::witness::Witness;
    use crate::persistence::data_cache::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::smart_contract::application_engine::TEST_MODE_GAS;
    use crate::smart_contract::native::{LedgerContract, NativeContract};
    use crate::smart_contract::trigger_type::TriggerType;
    use crate::UInt160;
    use crate::WitnessScope;
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

        let container: Arc<dyn crate::IVerifiable> =
            Arc::new(transaction.clone()) as Arc<dyn crate::IVerifiable>;
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
