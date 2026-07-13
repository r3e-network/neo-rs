//! Opt-in transaction trace formatting for native persistence diagnostics.
//!
//! These helpers are intentionally kept out of the hot persist body; they only
//! run when `NEO_TRACE_TX` matches the current transaction.

use neo_execution::ApplicationEngine;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::{UInt160, UInt256};

use super::artifacts::stack_value_snapshot;

#[derive(Debug, Clone, Default)]
pub(crate) struct TraceTxFilter {
    all: bool,
    hashes: Vec<String>,
}

impl TraceTxFilter {
    pub(crate) fn from_env() -> Self {
        Self::from_raw(std::env::var("NEO_TRACE_TX").ok().as_deref())
    }

    pub(crate) fn from_raw(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return Self::default();
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
        filter
    }

    pub(crate) fn matches(&self, tx_hash: &UInt256) -> bool {
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
                .map(|item| format!("{:?}", stack_value_snapshot(item)))
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
