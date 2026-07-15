//! Opt-in transaction trace formatting for native persistence diagnostics.
//!
//! These helpers are intentionally kept out of the hot persist body; they only
//! run when `NEO_TRACE_TX` matches the current transaction,
//! `NEO_TRACE_BLOCK` contains the current block, or the opt-in slow-transaction
//! profiler is enabled. Targeted opcode/stack profiling is selected with
//! `NEO_PROFILE_VM_TX` or `NEO_PROFILE_VM_BLOCK`.

use neo_execution::ApplicationEngine;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::ApplicationExecuted;
use neo_primitives::{UInt160, UInt256};
use neo_vm::{OpcodeClass, StackItem, StackItemRpcJson, VmError, VmExecutionProfile};
use serde_json::{Value, json};
use std::str::FromStr;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub(crate) struct TraceTxFilter {
    all: bool,
    hashes: Vec<String>,
    block_range: Option<(u32, u32)>,
}

impl TraceTxFilter {
    pub(crate) fn from_env() -> Self {
        static FILTER: OnceLock<TraceTxFilter> = OnceLock::new();
        FILTER
            .get_or_init(|| {
                Self::from_raw_parts(
                    std::env::var("NEO_TRACE_TX").ok().as_deref(),
                    std::env::var("NEO_TRACE_BLOCK").ok().as_deref(),
                )
            })
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn from_raw(raw: Option<&str>) -> Self {
        Self::from_raw_parts(raw, None)
    }

    pub(crate) fn from_raw_parts(tx_raw: Option<&str>, block_raw: Option<&str>) -> Self {
        let Some(raw) = tx_raw else {
            return Self {
                block_range: block_raw.and_then(parse_block_range),
                ..Self::default()
            };
        };
        let mut filter = Self::default();
        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" || entry.eq_ignore_ascii_case("all") {
                filter.all = true;
            } else {
                filter.hashes.push(entry.to_ascii_lowercase());
            }
        }
        filter.block_range = block_raw.and_then(parse_block_range);
        filter
    }

    pub(crate) fn matches(&self, block_index: u32, tx_hash: &UInt256) -> bool {
        if self
            .block_range
            .is_some_and(|(start, end)| (start..=end).contains(&block_index))
        {
            return true;
        }
        if self.all {
            return true;
        }
        if self.hashes.is_empty() {
            return false;
        }
        let tx_hash = tx_hash.to_string().to_ascii_lowercase();
        self.hashes.iter().any(|entry| entry == &tx_hash)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SlowTxFilter {
    threshold_us: Option<u64>,
    block_range: Option<(u32, u32)>,
}

impl SlowTxFilter {
    pub(crate) fn from_env() -> Self {
        static FILTER: OnceLock<SlowTxFilter> = OnceLock::new();
        *FILTER.get_or_init(|| {
            Self::from_raw_parts(
                std::env::var("NEO_PROFILE_SLOW_TX_US").ok().as_deref(),
                std::env::var("NEO_PROFILE_SLOW_TX_BLOCK").ok().as_deref(),
            )
        })
    }

    pub(crate) fn from_raw_parts(threshold: Option<&str>, block: Option<&str>) -> Self {
        let threshold_us = threshold
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|threshold| *threshold > 0);
        Self {
            threshold_us,
            block_range: block.and_then(parse_block_range),
        }
    }

    pub(crate) fn matches(self, block_index: u32, execute_us: u64) -> bool {
        self.threshold_us.is_some_and(|threshold| {
            execute_us >= threshold
                && self
                    .block_range
                    .is_none_or(|(start, end)| (start..=end).contains(&block_index))
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VmProfileFilter {
    all: bool,
    hashes: Vec<UInt256>,
    block_range: Option<(u32, u32)>,
}

impl VmProfileFilter {
    pub(crate) fn from_env() -> &'static Self {
        static FILTER: OnceLock<VmProfileFilter> = OnceLock::new();
        FILTER.get_or_init(|| {
            Self::from_raw_parts(
                std::env::var("NEO_PROFILE_VM_TX").ok().as_deref(),
                std::env::var("NEO_PROFILE_VM_BLOCK").ok().as_deref(),
            )
        })
    }

    pub(crate) fn from_raw_parts(tx_raw: Option<&str>, block_raw: Option<&str>) -> Self {
        let mut filter = Self {
            block_range: block_raw.and_then(parse_block_range),
            ..Self::default()
        };
        let Some(raw) = tx_raw else {
            return filter;
        };

        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" || entry.eq_ignore_ascii_case("all") {
                filter.all = true;
            } else if let Ok(hash) = UInt256::from_str(entry) {
                filter.hashes.push(hash);
            }
        }
        filter
    }

    pub(crate) fn matches(&self, block_index: u32, tx_hash: &UInt256) -> bool {
        self.block_range
            .is_some_and(|(start, end)| (start..=end).contains(&block_index))
            || self.all
            || self.hashes.contains(tx_hash)
    }
}

pub(crate) fn format_vm_opcode_classes(profile: &VmExecutionProfile) -> String {
    OpcodeClass::ALL
        .iter()
        .filter_map(|class| {
            let count = profile.opcode_class_count(*class);
            (count > 0).then(|| format!("{}={count}", class.name()))
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn format_vm_hottest_opcodes(profile: &VmExecutionProfile, limit: usize) -> String {
    profile
        .hottest_opcodes(limit)
        .into_iter()
        .map(|(opcode, count)| format!("{}={count}", opcode.name()))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_block_range(raw: &str) -> Option<(u32, u32)> {
    let raw = raw.trim();
    let (start, end) = match raw.split_once('-') {
        Some((start, end)) => (start.trim().parse().ok()?, end.trim().parse().ok()?),
        None => {
            let height = raw.parse().ok()?;
            (height, height)
        }
    };
    (start <= end).then_some((start, end))
}

pub(crate) fn trace_tx_artifact(
    block_index: u32,
    tx_hash: &UInt256,
    executed: &ApplicationExecuted,
) -> Result<Value, VmError> {
    let stack = executed
        .stack
        .iter()
        .map(|item| StackItemRpcJson::stack_item_rpc_json(item, None))
        .collect::<Result<Vec<_>, _>>()?;
    let notifications = executed
        .notifications
        .iter()
        .map(|notification| {
            Ok(json!({
                "contract": notification.script_hash.to_string(),
                "eventname": notification.event_name,
                "state": StackItemRpcJson::stack_item_rpc_json(
                    &StackItem::from_array(notification.state().to_vec()),
                    None,
                )?,
            }))
        })
        .collect::<Result<Vec<_>, VmError>>()?;

    Ok(json!({
        "block_index": block_index,
        "txid": tx_hash.to_string(),
        "executions": [{
            "trigger": executed.event_name(),
            "vmstate": executed.state(),
            "exception": executed.exception,
            "gasconsumed": executed.gas_consumed.to_string(),
            "stack": stack,
            "notifications": notifications,
        }],
    }))
}

pub(crate) fn trace_tx_frames<P, B>(
    engine: &ApplicationEngine<P, neo_execution::NoDiagnostic, B>,
) -> String
where
    P: NativeContractProvider + 'static,
    B: neo_storage::CacheRead,
{
    engine
        .invocation_stack()
        .iter()
        .enumerate()
        .map(|(index, context)| {
            let state_arc = context.state();
            let state = state_arc.lock();
            let script_hash = state
                .script_hash
                .or_else(|| UInt160::from_bytes(&context.script_hash()).ok())
                .map(|hash| hash.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let method = state.method_name.as_deref().unwrap_or("-");
            let opcode = context
                .current_instruction()
                .map(|instruction| format!("{:?}", instruction.opcode()))
                .unwrap_or_else(|_| "<none>".to_string());
            format!(
                "#{index}:hash={script_hash}:method={method}:ip={}:opcode={opcode}",
                context.instruction_pointer()
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn trace_tx_notifications<P, B>(
    engine: &ApplicationEngine<P, neo_execution::NoDiagnostic, B>,
) -> String
where
    P: NativeContractProvider + 'static,
    B: neo_storage::CacheRead,
{
    engine
        .notifications()
        .iter()
        .enumerate()
        .map(|(index, notification)| {
            let state = notification
                .state()
                .iter()
                .map(|item| format!("{item:?}"))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "#{index}:contract={}:event={}:state=[{}]",
                notification.script_hash, notification.event_name, state
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}
