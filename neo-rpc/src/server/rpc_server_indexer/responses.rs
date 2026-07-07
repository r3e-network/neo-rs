use neo_indexer::{
    AccountTransactionRecord, BlockIndexRecord, NotificationIndexRecord, TransactionIndexRecord,
};
use serde_json::{Value, json};

use super::RpcServerIndexer;

impl RpcServerIndexer {
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
