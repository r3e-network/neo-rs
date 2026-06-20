use std::str::FromStr;

use neo_indexer::NotificationIndexRecord;
use neo_payloads::Block;
use neo_primitives::{UInt160, UInt256};
use neo_rpc::application_logs::ApplicationLogsService;
use serde_json::Value;
use tracing::warn;

pub(super) fn recover_application_log_notifications(
    application_logs: Option<&ApplicationLogsService>,
    block: &Block,
    context: &'static str,
) -> Vec<NotificationIndexRecord> {
    let Some(logs) = application_logs else {
        return Vec::new();
    };
    match application_log_notification_records(logs, block) {
        Ok(records) => records,
        Err(err) => {
            warn!(
                target: "neo::indexer",
                block_height = block.index(),
                context,
                error = %err,
                "failed to recover indexer notifications from application logs"
            );
            Vec::new()
        }
    }
}

#[derive(Debug)]
pub(super) struct ApplicationLogExecution {
    pub(super) trigger: String,
    tx_hash: Option<UInt256>,
    notifications: Vec<ApplicationLogNotification>,
}

#[derive(Debug)]
struct ApplicationLogNotification {
    contract_hash: UInt160,
    event_name: String,
    state: Vec<Value>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum ApplicationLogRecoveryError {
    #[error("failed to hash block {height}: {reason}")]
    BlockHash { height: u32, reason: String },

    #[error("failed to hash transaction in block {height}: {reason}")]
    TransactionHash { height: u32, reason: String },

    #[error("application log contains too many executions")]
    TooManyExecutions,

    #[error("application log contains too many notifications")]
    TooManyNotifications,

    #[error("application log notification state is too large")]
    NotificationStateTooLarge,

    #[error("invalid transaction log txid '{text}': {reason}")]
    InvalidTransactionLogHash { text: String, reason: String },

    #[error("transaction log txid {observed} does not match block transaction {expected}")]
    TransactionLogHashMismatch {
        observed: UInt256,
        expected: UInt256,
    },

    #[error("application log missing executions array")]
    MissingExecutions,

    #[error("invalid notification contract '{text}': {reason}")]
    InvalidNotificationContract { text: String, reason: String },

    #[error("application log notification state array missing value")]
    MissingStateArrayValue,

    #[error("application log entry missing string field '{field}'")]
    MissingStringField { field: &'static str },
}

pub(super) fn application_log_notification_records(
    logs: &ApplicationLogsService,
    block: &Block,
) -> Result<Vec<NotificationIndexRecord>, ApplicationLogRecoveryError> {
    let block_hash = block
        .try_hash()
        .map_err(|err| ApplicationLogRecoveryError::BlockHash {
            height: block.index(),
            reason: err.to_string(),
        })?;
    let mut pre_transaction_executions = Vec::new();
    let mut post_transaction_executions = Vec::new();

    if let Some(block_log) = logs.get_block_log(&block_hash) {
        for execution in parse_application_log_executions(&block_log, None)? {
            if execution.trigger == "OnPersist" {
                pre_transaction_executions.push(execution);
            } else {
                post_transaction_executions.push(execution);
            }
        }
    }

    let mut ordered_executions = pre_transaction_executions;
    for transaction in &block.transactions {
        let tx_hash =
            transaction
                .try_hash()
                .map_err(|err| ApplicationLogRecoveryError::TransactionHash {
                    height: block.index(),
                    reason: err.to_string(),
                })?;
        if let Some(tx_log) = logs.get_transaction_log(&tx_hash) {
            validate_transaction_log_hash(&tx_log, tx_hash)?;
            ordered_executions.extend(parse_application_log_executions(&tx_log, Some(tx_hash))?);
        }
    }
    ordered_executions.extend(post_transaction_executions);

    let mut records = Vec::new();
    for (execution_position, execution) in ordered_executions.into_iter().enumerate() {
        let execution_index = u32::try_from(execution_position)
            .map_err(|_| ApplicationLogRecoveryError::TooManyExecutions)?;
        for (notification_position, notification) in execution.notifications.into_iter().enumerate()
        {
            let notification_index = u32::try_from(notification_position)
                .map_err(|_| ApplicationLogRecoveryError::TooManyNotifications)?;
            let state_item_count = u32::try_from(notification.state.len())
                .map_err(|_| ApplicationLogRecoveryError::NotificationStateTooLarge)?;
            records.push(NotificationIndexRecord {
                block_hash,
                block_height: block.index(),
                tx_hash: execution.tx_hash,
                execution_index,
                notification_index,
                contract_hash: notification.contract_hash,
                event_name: notification.event_name,
                trigger: execution.trigger.clone(),
                state_item_count,
                state: notification.state,
                accounts: Vec::new(),
            });
        }
    }

    Ok(records)
}

fn validate_transaction_log_hash(
    log: &Value,
    expected: UInt256,
) -> Result<(), ApplicationLogRecoveryError> {
    let Some(text) = log.get("txid").and_then(Value::as_str) else {
        return Ok(());
    };
    let observed = UInt256::from_str(text).map_err(|err| {
        ApplicationLogRecoveryError::InvalidTransactionLogHash {
            text: text.to_string(),
            reason: err.to_string(),
        }
    })?;
    if observed != expected {
        return Err(ApplicationLogRecoveryError::TransactionLogHashMismatch { observed, expected });
    }
    Ok(())
}

pub(super) fn parse_application_log_executions(
    log: &Value,
    tx_hash: Option<UInt256>,
) -> Result<Vec<ApplicationLogExecution>, ApplicationLogRecoveryError> {
    let executions = log
        .get("executions")
        .and_then(Value::as_array)
        .ok_or(ApplicationLogRecoveryError::MissingExecutions)?;
    executions
        .iter()
        .map(|execution| parse_application_log_execution(execution, tx_hash))
        .collect()
}

fn parse_application_log_execution(
    execution: &Value,
    tx_hash: Option<UInt256>,
) -> Result<ApplicationLogExecution, ApplicationLogRecoveryError> {
    let trigger = required_string(execution, "trigger")?.to_string();
    let notifications = match execution.get("notifications").and_then(Value::as_array) {
        Some(notifications) => notifications
            .iter()
            .map(parse_application_log_notification)
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(ApplicationLogExecution {
        trigger,
        tx_hash,
        notifications,
    })
}

fn parse_application_log_notification(
    notification: &Value,
) -> Result<ApplicationLogNotification, ApplicationLogRecoveryError> {
    let contract = required_string(notification, "contract")?;
    let contract_hash = UInt160::from_str(contract).map_err(|err| {
        ApplicationLogRecoveryError::InvalidNotificationContract {
            text: contract.to_string(),
            reason: err.to_string(),
        }
    })?;
    let event_name = required_string(notification, "eventname")?.to_string();
    let state = notification
        .get("state")
        .map(application_log_state_items)
        .transpose()?
        .unwrap_or_default();
    Ok(ApplicationLogNotification {
        contract_hash,
        event_name,
        state,
    })
}

fn application_log_state_items(state: &Value) -> Result<Vec<Value>, ApplicationLogRecoveryError> {
    let Some(object) = state.as_object() else {
        return Ok(Vec::new());
    };
    if object.get("type").and_then(Value::as_str) != Some("Array") {
        return Ok(Vec::new());
    }
    let items = object
        .get("value")
        .and_then(Value::as_array)
        .ok_or(ApplicationLogRecoveryError::MissingStateArrayValue)?;
    Ok(items.clone())
}

fn required_string<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a str, ApplicationLogRecoveryError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(ApplicationLogRecoveryError::MissingStringField { field })
}
