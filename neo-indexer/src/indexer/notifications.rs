use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_payloads::ApplicationExecuted;
use neo_primitives::{UInt160, UInt256};
use neo_vm::{StackItem, rpc_json::StackItemRpcJson};

use crate::error::{IndexerError, IndexerResult};
use crate::model::{BlockIndexRecord, NotificationIndexRecord, TransactionIndexRecord};

pub(super) fn prepare_notifications(
    block: &BlockIndexRecord,
    block_transactions: &HashSet<UInt256>,
    executions: &[ApplicationExecuted],
) -> IndexerResult<Vec<NotificationIndexRecord>> {
    let mut notifications = Vec::new();
    for (execution_position, execution) in executions.iter().enumerate() {
        let execution_index =
            u32::try_from(execution_position).map_err(|_| IndexerError::TooManyExecutions {
                count: executions.len(),
            })?;
        let tx_hash = execution
            .transaction
            .as_ref()
            .map(|transaction| {
                transaction
                    .try_hash()
                    .map_err(|source| IndexerError::ExecutionTransactionHash {
                        execution_index,
                        source,
                    })
            })
            .transpose()?;
        if let Some(hash) = tx_hash {
            if !block_transactions.contains(&hash) {
                return Err(IndexerError::ExecutionTransactionNotInBlock {
                    execution_index,
                    hash,
                });
            }
        }

        for (notification_position, notification) in execution.notifications.iter().enumerate() {
            let notification_index = u32::try_from(notification_position).map_err(|_| {
                IndexerError::TooManyNotifications {
                    execution_index,
                    count: execution.notifications.len(),
                }
            })?;
            let state_item_count = u32::try_from(notification.state.len()).map_err(|_| {
                IndexerError::TooManyNotificationStateItems {
                    execution_index,
                    notification_index,
                    count: notification.state.len(),
                }
            })?;
            let state = notification
                .state
                .iter()
                .map(|item| {
                    StackItemRpcJson::stack_item_rpc_json(item, None).map_err(|source| {
                        IndexerError::NotificationStateJson {
                            execution_index,
                            notification_index,
                            source,
                        }
                    })
                })
                .collect::<IndexerResult<Vec<_>>>()?;
            let accounts = transfer_participant_accounts(
                &notification.event_name,
                notification.state.as_slice(),
            );

            notifications.push(NotificationIndexRecord {
                block_hash: block.hash,
                block_height: block.height,
                tx_hash,
                execution_index,
                notification_index,
                contract_hash: notification.script_hash,
                event_name: notification.event_name.clone(),
                trigger: execution.event_name().to_string(),
                state_item_count,
                state,
                accounts,
            });
        }
    }
    Ok(notifications)
}

pub(super) fn normalize_notification_records(
    block: &BlockIndexRecord,
    transactions: &[TransactionIndexRecord],
    notifications: Vec<NotificationIndexRecord>,
) -> IndexerResult<Vec<NotificationIndexRecord>> {
    let transaction_hashes = transactions
        .iter()
        .map(|transaction| transaction.hash)
        .collect::<HashSet<_>>();
    let mut seen_notifications = HashSet::with_capacity(notifications.len());
    let mut normalized = Vec::with_capacity(notifications.len());

    for mut notification in notifications {
        if notification.block_hash != block.hash {
            return Err(IndexerError::MissingNotificationBlock {
                block_hash: notification.block_hash,
                block_height: notification.block_height,
                execution_index: notification.execution_index,
                notification_index: notification.notification_index,
            });
        }
        if notification.block_height != block.height {
            return Err(IndexerError::NotificationBlockHeightMismatch {
                block_hash: notification.block_hash,
                notification_height: notification.block_height,
                block_height: block.height,
                execution_index: notification.execution_index,
                notification_index: notification.notification_index,
            });
        }
        if !seen_notifications.insert((
            notification.block_hash,
            notification.execution_index,
            notification.notification_index,
        )) {
            return Err(IndexerError::DuplicateNotification {
                block_hash: notification.block_hash,
                execution_index: notification.execution_index,
                notification_index: notification.notification_index,
            });
        }
        if let Some(tx_hash) = notification.tx_hash {
            if !transaction_hashes.contains(&tx_hash) {
                return Err(IndexerError::MissingNotificationTransaction {
                    tx_hash,
                    block_hash: notification.block_hash,
                    execution_index: notification.execution_index,
                    notification_index: notification.notification_index,
                });
            }
        }
        normalize_notification_payload_metadata(&mut notification)?;
        normalized.push(notification);
    }

    Ok(normalized)
}

pub(super) fn sort_notifications(records: &mut [NotificationIndexRecord]) {
    records.sort_by_key(|record| {
        (
            record.block_height,
            record.execution_index,
            record.notification_index,
        )
    });
}

pub(super) fn normalize_notification_payload_metadata(
    record: &mut NotificationIndexRecord,
) -> IndexerResult<()> {
    record.state_item_count = u32::try_from(record.state.len()).map_err(|_| {
        IndexerError::TooManyNotificationStateItems {
            execution_index: record.execution_index,
            notification_index: record.notification_index,
            count: record.state.len(),
        }
    })?;
    record.accounts = normalize_notification_accounts(record);
    Ok(())
}

fn transfer_participant_accounts(event_name: &str, state_items: &[StackItem]) -> Vec<UInt160> {
    if event_name != "Transfer" || state_items.len() < 3 {
        return Vec::new();
    }

    normalize_accounts_in_event_order(state_items.iter().take(2).filter_map(transfer_account))
}

fn transfer_participant_accounts_from_json(record: &NotificationIndexRecord) -> Vec<UInt160> {
    if record.event_name != "Transfer" || record.state.len() < 3 {
        return Vec::new();
    }

    normalize_accounts_in_event_order(
        record
            .state
            .iter()
            .take(2)
            .filter_map(json_transfer_account),
    )
}

fn normalize_notification_accounts(record: &NotificationIndexRecord) -> Vec<UInt160> {
    let accounts = if record.event_name == "Transfer" && record.state.len() >= 3 {
        transfer_participant_accounts_from_json(record)
    } else if record.accounts.is_empty() {
        Vec::new()
    } else {
        record.accounts.clone()
    };
    normalize_accounts_in_event_order(accounts)
}

fn normalize_accounts_in_event_order(accounts: impl IntoIterator<Item = UInt160>) -> Vec<UInt160> {
    let mut seen = HashSet::new();
    accounts
        .into_iter()
        .filter(|account| *account != UInt160::zero())
        .filter(|account| seen.insert(*account))
        .collect()
}

fn json_transfer_account(value: &serde_json::Value) -> Option<UInt160> {
    let object = value.as_object()?;
    match object.get("type")?.as_str()? {
        "Any" => None,
        "ByteString" => {
            let encoded = object.get("value")?.as_str()?;
            let bytes = BASE64_STANDARD.decode(encoded).ok()?;
            if bytes.len() != UInt160::LENGTH {
                return None;
            }
            UInt160::from_bytes(&bytes).ok()
        }
        _ => None,
    }
}

fn transfer_account(item: &StackItem) -> Option<UInt160> {
    if item.is_null() {
        return None;
    }
    let bytes = item.as_bytes().ok()?;
    if bytes.len() != UInt160::LENGTH {
        return None;
    }
    UInt160::from_bytes(&bytes).ok()
}
