use super::models::{ApplicationEngineLogModel, BlockchainEventModel, BlockchainExecutionModel};
use crate::application_logs::store::log_storage_store::LogStorageStore;
use crate::application_logs::store::states::{
    BlockLogState, ContractLogState, EngineLogState, ExecutionLogState, NotifyLogState,
    TransactionEngineLogState, TransactionLogState,
};
use neo_core::neo_io::{IoError, IoResult};
use neo_core::neo_ledger::{ApplicationExecuted, Block};
use neo_core::persistence::i_store::IStore;
use neo_core::persistence::i_store_snapshot::IStoreSnapshot;
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::smart_contract::{LogEventArgs, TriggerType};
use neo_core::{UInt160, UInt256};
use neo_vm::StackItem;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ContractLogEntry {
    pub event: BlockchainEventModel,
    pub transaction_hash: UInt256,
    pub trigger: TriggerType,
    pub timestamp: u64,
    pub notification_index: u32,
}

pub struct NeoStore {
    store: Arc<dyn IStore>,
    block_snapshot: Option<Arc<dyn IStoreSnapshot>>,
}

impl NeoStore {
    pub fn new(store: Arc<dyn IStore>) -> Self {
        Self {
            store,
            block_snapshot: None,
        }
    }

    pub fn start_block_log_batch(&mut self) {
        if let Some(snapshot) = self.block_snapshot.take() {
            let mut storage = LogStorageStore::new(snapshot);
            storage.commit();
        }
        self.block_snapshot = Some(self.store.get_snapshot());
    }

    pub fn commit_block_log(&mut self) {
        if let Some(snapshot) = self.block_snapshot.take() {
            let mut storage = LogStorageStore::new(snapshot);
            storage.commit();
        }
    }

    pub fn get_contract_log(
        &self,
        script_hash: &UInt160,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogEntry>> {
        self.get_contract_log_internal(script_hash, None, None, page, page_size)
    }

    pub fn get_contract_log_with_trigger(
        &self,
        script_hash: &UInt160,
        trigger: TriggerType,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogEntry>> {
        self.get_contract_log_internal(script_hash, Some(trigger), None, page, page_size)
    }

    pub fn get_contract_log_with_trigger_and_event(
        &self,
        script_hash: &UInt160,
        trigger: TriggerType,
        event_name: &str,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogEntry>> {
        self.get_contract_log_internal(
            script_hash,
            Some(trigger),
            Some(event_name),
            page,
            page_size,
        )
    }

    pub fn put_transaction_engine_log_state(
        &mut self,
        hash: &UInt256,
        logs: &[LogEventArgs],
    ) -> IoResult<()> {
        let snapshot = Arc::clone(
            self.block_snapshot
                .as_ref()
                .ok_or_else(|| IoError::invalid_data("Block snapshot not initialised"))?,
        );
        let mut storage = LogStorageStore::new(snapshot);
        let mut log_ids = Vec::with_capacity(logs.len());
        for log in logs {
            let state = EngineLogState::create(log.script_hash, log.message.clone());
            log_ids.push(storage.put_engine_state(&state)?);
        }
        let state = TransactionEngineLogState::create(log_ids);
        storage.put_transaction_engine_state(hash, &state)
    }

    pub fn put_block_log(
        &mut self,
        block: &Block,
        executed_list: &[ApplicationExecuted],
    ) -> IoResult<()> {
        let snapshot = Arc::clone(
            self.block_snapshot
                .as_ref()
                .ok_or_else(|| IoError::invalid_data("Block snapshot not initialised"))?,
        );

        for executed in executed_list {
            let mut storage = LogStorageStore::new(Arc::clone(&snapshot));
            let execution_state_id = Self::put_execution_log_block(&mut storage, block, executed)?;
            Self::put_block_and_transaction_log(&mut storage, block, executed, execution_state_id)?;
        }
        Ok(())
    }

    pub fn get_block_log(
        &self,
        hash: &UInt256,
        trigger: TriggerType,
    ) -> IoResult<Option<BlockchainExecutionModel>> {
        self.get_block_log_internal(hash, trigger, None)
    }

    pub fn get_block_log_with_event(
        &self,
        hash: &UInt256,
        trigger: TriggerType,
        event_name: &str,
    ) -> IoResult<Option<BlockchainExecutionModel>> {
        self.get_block_log_internal(hash, trigger, Some(event_name))
    }

    pub fn get_transaction_log(
        &self,
        hash: &UInt256,
    ) -> IoResult<Option<BlockchainExecutionModel>> {
        self.get_transaction_log_internal(hash, None)
    }

    pub fn get_transaction_log_with_event(
        &self,
        hash: &UInt256,
        event_name: &str,
    ) -> IoResult<Option<BlockchainExecutionModel>> {
        self.get_transaction_log_internal(hash, Some(event_name))
    }

    fn get_contract_log_internal(
        &self,
        script_hash: &UInt160,
        trigger: Option<TriggerType>,
        event_name: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> IoResult<Vec<ContractLogEntry>> {
        let mut storage = LogStorageStore::new(self.store.get_snapshot());
        let records = match (trigger, event_name) {
            (Some(t), Some(event)) => storage.find_contract_state_with_trigger_and_event(
                script_hash,
                t,
                event,
                page,
                page_size,
            )?,
            (Some(t), None) => {
                storage.find_contract_state_with_trigger(script_hash, t, page, page_size)?
            }
            (None, _) => storage.find_contract_state(script_hash, page, page_size)?,
        };

        let mut entries = Vec::with_capacity(records.len());
        for record in records {
            let stack =
                Self::create_stack_item_array(&mut storage, &record.state.notify.stack_item_ids)?;
            let event = BlockchainEventModel::create_from_contract_state(&record.state, stack);
            entries.push(ContractLogEntry {
                event,
                transaction_hash: record.state.transaction_hash,
                trigger: record.state.trigger,
                timestamp: record.timestamp,
                notification_index: record.notification_index,
            });
        }

        Ok(entries)
    }

    fn get_block_log_internal(
        &self,
        hash: &UInt256,
        trigger: TriggerType,
        event_filter: Option<&str>,
    ) -> IoResult<Option<BlockchainExecutionModel>> {
        let mut storage = LogStorageStore::new(self.store.get_snapshot());
        let execution_id = match storage.try_get_execution_block_state(hash, trigger)? {
            Some(id) => id,
            None => return Ok(None),
        };

        let execution_state = match storage.try_get_execution_state(execution_id)? {
            Some(state) => state,
            None => return Ok(None),
        };

        let stack = Self::create_stack_item_array(&mut storage, &execution_state.stack_item_ids)?;
        let mut model = BlockchainExecutionModel::create(trigger, &execution_state, stack);

        if let Some(block_state) = storage.try_get_block_state(hash, trigger)? {
            let mut notifications = Vec::new();
            for notify_id in block_state.notify_log_ids {
                if let Some(notify_state) = storage.try_get_notify_state(notify_id)? {
                    if let Some(filter) = event_filter {
                        if !notify_state.event_name.eq_ignore_ascii_case(filter) {
                            continue;
                        }
                    }
                    let stack =
                        Self::create_stack_item_array(&mut storage, &notify_state.stack_item_ids)?;
                    notifications.push(BlockchainEventModel::create_from_notify_state(
                        &notify_state,
                        stack,
                    ));
                }
            }
            model = model.with_notifications(notifications);
        }

        Ok(Some(model))
    }

    fn get_transaction_log_internal(
        &self,
        hash: &UInt256,
        event_filter: Option<&str>,
    ) -> IoResult<Option<BlockchainExecutionModel>> {
        let mut storage = LogStorageStore::new(self.store.get_snapshot());
        let execution_id = match storage.try_get_execution_transaction_state(hash)? {
            Some(id) => id,
            None => return Ok(None),
        };

        let execution_state = match storage.try_get_execution_state(execution_id)? {
            Some(state) => state,
            None => return Ok(None),
        };

        let stack = Self::create_stack_item_array(&mut storage, &execution_state.stack_item_ids)?;
        let mut model =
            BlockchainExecutionModel::create(TriggerType::Application, &execution_state, stack);

        if let Some(transaction_state) = storage.try_get_transaction_state(hash)? {
            let mut notifications = Vec::new();
            for notify_id in transaction_state.notify_log_ids {
                if let Some(notify_state) = storage.try_get_notify_state(notify_id)? {
                    if let Some(filter) = event_filter {
                        if !notify_state.event_name.eq_ignore_ascii_case(filter) {
                            continue;
                        }
                    }
                    let stack =
                        Self::create_stack_item_array(&mut storage, &notify_state.stack_item_ids)?;
                    notifications.push(BlockchainEventModel::create_from_notify_state(
                        &notify_state,
                        stack,
                    ));
                }
            }
            model = model.with_notifications(notifications);
        }

        if let Some(engine_state) = storage.try_get_transaction_engine_state(hash)? {
            let mut logs = Vec::new();
            for log_id in engine_state.log_ids {
                if let Some(log_state) = storage.try_get_engine_state(log_id)? {
                    logs.push(ApplicationEngineLogModel::create(&log_state));
                }
            }
            model = model.with_logs(logs);
        }

        Ok(Some(model))
    }

    fn put_execution_log_block(
        storage: &mut LogStorageStore,
        block: &Block,
        executed: &ApplicationExecuted,
    ) -> IoResult<Uuid> {
        let stack_ids = Self::create_stack_item_id_list(storage, &executed.stack)?;
        let execution_state = ExecutionLogState::create(executed, stack_ids);
        let execution_state_id = storage.put_execution_state(&execution_state)?;

        let header = block.header.clone();
        let block_hash = header.hash();
        storage.put_execution_block_state(&block_hash, executed.trigger, execution_state_id)?;
        Ok(execution_state_id)
    }

    fn put_block_and_transaction_log(
        storage: &mut LogStorageStore,
        block: &Block,
        executed: &ApplicationExecuted,
        execution_state_id: Uuid,
    ) -> IoResult<()> {
        let mut notify_ids = Vec::new();

        if let Some(tx) = &executed.transaction {
            storage.put_execution_transaction_state(&tx.hash(), execution_state_id)?;
        }

        for (index, notify) in executed.notifications.iter().enumerate() {
            let notify_stack_ids = Self::create_stack_item_id_list_from_notify(storage, notify)?;
            let notify_state = NotifyLogState::create(
                notify.script_hash,
                notify.event_name.clone(),
                notify_stack_ids.clone(),
            );
            let contract_state = ContractLogState::create(
                executed.transaction.as_ref().map(|tx| tx.hash()),
                executed.trigger,
                notify_state.clone(),
            );

            storage.put_contract_state(
                &notify.script_hash,
                block.header.timestamp,
                index as u32,
                &contract_state,
            )?;

            let notify_id = storage.put_notify_state(&notify_state)?;
            notify_ids.push(notify_id);
        }

        if let Some(tx) = &executed.transaction {
            let transaction_state = TransactionLogState::create(notify_ids.clone());
            storage.put_transaction_state(&tx.hash(), &transaction_state)?;
        }

        let header = block.header.clone();
        let block_hash = header.hash();
        let block_state = BlockLogState::create(notify_ids);
        storage.put_block_state(&block_hash, executed.trigger, &block_state)
    }

    fn create_stack_item_id_list(
        storage: &mut LogStorageStore,
        items: &[StackItem],
    ) -> IoResult<Vec<Uuid>> {
        let mut ids = Vec::with_capacity(items.len());
        for item in items {
            ids.push(storage.put_stack_item_state(item)?);
        }
        Ok(ids)
    }

    fn create_stack_item_id_list_from_notify(
        storage: &mut LogStorageStore,
        notify: &NotifyEventArgs,
    ) -> IoResult<Vec<Uuid>> {
        let mut ids = Vec::with_capacity(notify.state.len());
        for item in &notify.state {
            ids.push(storage.put_stack_item_state(item)?);
        }
        Ok(ids)
    }

    fn create_stack_item_array(
        storage: &mut LogStorageStore,
        ids: &[Uuid],
    ) -> IoResult<Vec<StackItem>> {
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(item) = storage.try_get_stack_item_state(*id)? {
                items.push(item);
            }
        }
        Ok(items)
    }
}
