use neo_indexer::{
    AccountTransactionRecord, BlockIndexRecord, NotificationIndexRecord, TransactionIndexRecord,
};
use serde_json::{Value, json};

use super::RpcServerIndexer;

impl RpcServerIndexer {
    pub(super) fn block_to_json(record: BlockIndexRecord) -> Value {
        json!({
            "hash": record.hash.to_string(),
            "height": record.height,
            "time": record.timestamp,
            "txcount": record.transaction_count,
        })
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
}
