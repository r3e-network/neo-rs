use neo_indexer::{
    AccountTransactionRecord, BlockIndexRecord, IndexerService, IndexerStatus,
    NotificationIndexRecord, TransactionIndexRecord,
};
use serde_json::{Value, json};

use super::RpcServerIndexer;

pub(super) struct ApplicationLogsStatus {
    pub(super) enabled: bool,
    pub(super) notification_recovery: bool,
    pub(super) path: Option<String>,
    pub(super) debug: Option<bool>,
}

impl RpcServerIndexer {
    pub(super) fn indexer_status_to_json(
        service: &IndexerService,
        status: IndexerStatus,
        ledger_height: Option<u32>,
        application_logs: ApplicationLogsStatus,
    ) -> Value {
        json!({
            "indexedheight": status.indexed_height,
            "indexedhash": status.indexed_hash.map(|hash| hash.to_string()),
            "indexedblocks": status.indexed_blocks,
            "indexedtransactions": status.indexed_transactions,
            "indexedaccounts": status.indexed_accounts,
            "indexednotifications": status.indexed_notifications,
            "indexednotificationaccounts": status.indexed_notification_accounts,
            "ledgerheight": ledger_height,
            "blocksbehind": status.blocks_behind(ledger_height),
            "synced": status.is_synced_with(ledger_height),
            "applicationlogs": Self::application_logs_status_to_json(application_logs),
            "persistent": service.is_persistent(),
            "persistencemode": service.persistence_mode(),
            "snapshotpath": service.snapshot_path().map(|path| path.display().to_string()),
            "storepath": service.store_path().map(|path| path.display().to_string()),
        })
    }

    fn application_logs_status_to_json(status: ApplicationLogsStatus) -> Value {
        match status {
            ApplicationLogsStatus {
                enabled: true,
                notification_recovery,
                path,
                debug,
            } => json!({
                "enabled": true,
                "notificationrecovery": notification_recovery,
                "path": path.unwrap_or_default(),
                "debug": debug.unwrap_or(false),
            }),
            ApplicationLogsStatus { .. } => json!({
                "enabled": false,
                "notificationrecovery": false,
            }),
        }
    }

    pub(super) fn optional_block_to_json(record: Option<BlockIndexRecord>) -> Value {
        record.map_or(Value::Null, Self::block_to_json)
    }

    pub(super) fn blocks_to_json(records: Vec<BlockIndexRecord>) -> Value {
        Value::Array(records.into_iter().map(Self::block_to_json).collect())
    }

    pub(super) fn block_to_json(record: BlockIndexRecord) -> Value {
        json!({
            "hash": record.hash.to_string(),
            "height": record.height,
            "time": record.timestamp,
            "txcount": record.transaction_count,
        })
    }

    pub(super) fn optional_transaction_to_json(
        record: Option<TransactionIndexRecord>,
        address_version: u8,
    ) -> Value {
        record
            .map(|record| Self::transaction_to_json(&record, address_version))
            .unwrap_or(Value::Null)
    }

    pub(super) fn transactions_to_json(
        records: Vec<TransactionIndexRecord>,
        address_version: u8,
    ) -> Value {
        Value::Array(
            records
                .into_iter()
                .map(|record| Self::transaction_to_json(&record, address_version))
                .collect(),
        )
    }

    pub(super) fn transaction_to_json(
        record: &TransactionIndexRecord,
        address_version: u8,
    ) -> Value {
        let signers = record
            .signers
            .iter()
            .map(|account| {
                json!({
                    "account": account.to_string(),
                    "address": account.to_address_with_version(address_version),
                })
            })
            .collect::<Vec<_>>();
        json!({
            "txid": record.hash.to_string(),
            "blockhash": record.block_hash.to_string(),
            "blockheight": record.block_height,
            "txindex": record.transaction_index,
            "signers": signers,
        })
    }

    pub(super) fn account_transactions_to_json(
        records: Vec<AccountTransactionRecord>,
        address_version: u8,
    ) -> Value {
        Value::Array(
            records
                .into_iter()
                .map(|record| Self::account_transaction_to_json(&record, address_version))
                .collect(),
        )
    }

    pub(super) fn account_transaction_to_json(
        record: &AccountTransactionRecord,
        address_version: u8,
    ) -> Value {
        json!({
            "account": record.account.to_string(),
            "address": record.account.to_address_with_version(address_version),
            "txid": record.tx_hash.to_string(),
            "blockhash": record.block_hash.to_string(),
            "blockheight": record.block_height,
            "txindex": record.transaction_index,
        })
    }

    pub(super) fn notifications_to_json(
        records: Vec<NotificationIndexRecord>,
        address_version: u8,
    ) -> Value {
        Value::Array(
            records
                .into_iter()
                .map(|record| Self::notification_to_json(&record, address_version))
                .collect(),
        )
    }

    pub(super) fn notification_to_json(
        record: &NotificationIndexRecord,
        address_version: u8,
    ) -> Value {
        let tx_hash = record
            .tx_hash
            .map(|hash| Value::String(hash.to_string()))
            .unwrap_or(Value::Null);
        json!({
            "blockhash": record.block_hash.to_string(),
            "blockheight": record.block_height,
            "txid": tx_hash,
            "executionindex": record.execution_index,
            "notificationindex": record.notification_index,
            "contract": record.contract_hash.to_string(),
            "contractaddress": record.contract_hash.to_address_with_version(address_version),
            "eventname": record.event_name,
            "trigger": record.trigger,
            "stateitemcount": record.state_item_count,
            "state": record.state.clone(),
            "accounts": record.accounts.iter().map(|account| {
                json!({
                    "account": account.to_string(),
                    "address": account.to_address_with_version(address_version),
                })
            }).collect::<Vec<_>>(),
        })
    }

    pub(super) fn empty_list_to_json() -> Value {
        Value::Array(Vec::new())
    }
}
