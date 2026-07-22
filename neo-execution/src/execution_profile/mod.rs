//! # neo-execution::execution_profile
//!
//! Bounded application-context metadata for opt-in VM profiling.
//!
//! ## Boundary
//!
//! This module observes loaded application contexts and emits bounded profile
//! records. It does not cache execution results or affect VM control flow.
//!
//! ## Contents
//!
//! - Stable application-context identity records.
//! - Bounded per-transaction profile collection.
//! - Deterministic profile serialization for performance analysis.

use crate::ApplicationExecutionContext;
use neo_primitives::UInt160;
use neo_storage::CacheRead;
use serde::Serialize;

/// Maximum distinct application context identities retained per transaction.
pub const MAX_APPLICATION_CONTEXT_PROFILES: usize = 128;

/// One versioned logical contract/method identity mapped to raw VM bytecode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApplicationContextProfileEntry {
    /// Hash160 of the exact VM bytecode bytes.
    pub raw_script_hash: String,
    /// Exact byte length used to disambiguate and audit the raw fingerprint.
    pub raw_script_bytes: usize,
    /// Instruction offset at which this application context was loaded.
    pub entry_offset: usize,
    /// Logical contract or witness hash exposed by ApplicationEngine.
    pub logical_script_hash: String,
    /// Persisted contract ID, when this is a deployed contract context.
    pub contract_id: Option<i32>,
    /// Persisted update counter for the deployed contract version.
    pub contract_update_counter: Option<u16>,
    /// NEF checksum for the deployed contract version.
    pub nef_checksum: Option<u32>,
    /// Contract manifest name, when available.
    pub manifest_name: Option<String>,
    /// ABI method resolved from the exact entry offset, when available.
    pub method: Option<String>,
    /// Number of invocation arguments supplied to the context.
    pub argument_count: usize,
    /// Declared ABI parameter types retained by the context.
    pub parameter_types: Vec<String>,
    /// Declared ABI return type retained by the context.
    pub return_type: Option<String>,
    /// Effective Neo call-flag bits for the context.
    pub call_flags: u8,
    /// Whether ApplicationEngine classified this as a dynamic contract call.
    pub dynamic_call: bool,
    /// Loads of this exact identity during the profiled transaction.
    pub context_loads: u64,
}

impl ApplicationContextProfileEntry {
    fn from_context<B: CacheRead>(context: &ApplicationExecutionContext<B>) -> Self {
        let entry_offset = context.instruction_pointer();
        let raw_script_hash = UInt160::from(context.script_hash()).to_string();
        let state = context.state();
        let state = state.lock();
        let contract = state.contract.as_deref();
        let method = contract
            .and_then(|contract| {
                contract
                    .manifest
                    .abi
                    .methods
                    .iter()
                    .find(|method| usize::try_from(method.offset).ok() == Some(entry_offset))
                    .map(|method| method.name.clone())
            })
            .or_else(|| state.method_name.clone());
        Self {
            raw_script_hash,
            raw_script_bytes: context.script().len(),
            entry_offset,
            logical_script_hash: state
                .script_hash
                .map(|hash| hash.to_string())
                .unwrap_or_else(|| UInt160::from(context.script_hash()).to_string()),
            contract_id: contract.map(|contract| contract.id),
            contract_update_counter: contract.map(|contract| contract.update_counter),
            nef_checksum: contract.map(|contract| contract.nef.checksum),
            manifest_name: contract.map(|contract| contract.manifest.name.clone()),
            method,
            argument_count: state.argument_count,
            parameter_types: state
                .parameter_types
                .iter()
                .map(|parameter| format!("{parameter:?}"))
                .collect(),
            return_type: state
                .return_type
                .map(|return_type| format!("{return_type:?}")),
            call_flags: state.call_flags.bits(),
            dynamic_call: state.is_dynamic_call,
            context_loads: 1,
        }
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.raw_script_hash == other.raw_script_hash
            && self.raw_script_bytes == other.raw_script_bytes
            && self.entry_offset == other.entry_offset
            && self.logical_script_hash == other.logical_script_hash
            && self.contract_id == other.contract_id
            && self.contract_update_counter == other.contract_update_counter
            && self.nef_checksum == other.nef_checksum
            && self.manifest_name == other.manifest_name
            && self.method == other.method
            && self.argument_count == other.argument_count
            && self.parameter_types == other.parameter_types
            && self.return_type == other.return_type
            && self.call_flags == other.call_flags
            && self.dynamic_call == other.dynamic_call
    }
}

/// Deterministic snapshot emitted only for explicitly profiled transactions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApplicationContextProfile {
    /// Maximum distinct identities retained by the collector.
    pub context_capacity: usize,
    /// Retained context identities in deterministic frequency order.
    pub contexts: Vec<ApplicationContextProfileEntry>,
    /// Loads omitted after the distinct-identity bound was reached.
    pub other_context_loads: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ApplicationContextProfileCollector {
    contexts: Vec<ApplicationContextProfileEntry>,
    other_context_loads: u64,
}

impl ApplicationContextProfileCollector {
    pub(crate) fn record<B: CacheRead>(&mut self, context: &ApplicationExecutionContext<B>) {
        let candidate = ApplicationContextProfileEntry::from_context(context);
        if let Some(existing) = self
            .contexts
            .iter_mut()
            .find(|existing| existing.same_identity(&candidate))
        {
            existing.context_loads = existing.context_loads.saturating_add(1);
        } else if self.contexts.len() < MAX_APPLICATION_CONTEXT_PROFILES {
            self.contexts.push(candidate);
        } else {
            self.other_context_loads = self.other_context_loads.saturating_add(1);
        }
    }

    pub(crate) fn snapshot(&self) -> ApplicationContextProfile {
        let mut contexts = self.contexts.clone();
        contexts.sort_unstable_by(|left, right| {
            right
                .context_loads
                .cmp(&left.context_loads)
                .then_with(|| left.logical_script_hash.cmp(&right.logical_script_hash))
                .then_with(|| left.raw_script_hash.cmp(&right.raw_script_hash))
                .then_with(|| left.raw_script_bytes.cmp(&right.raw_script_bytes))
                .then_with(|| left.entry_offset.cmp(&right.entry_offset))
                .then_with(|| left.method.cmp(&right.method))
                .then_with(|| left.contract_id.cmp(&right.contract_id))
                .then_with(|| {
                    left.contract_update_counter
                        .cmp(&right.contract_update_counter)
                })
                .then_with(|| left.nef_checksum.cmp(&right.nef_checksum))
                .then_with(|| left.manifest_name.cmp(&right.manifest_name))
                .then_with(|| left.argument_count.cmp(&right.argument_count))
                .then_with(|| left.parameter_types.cmp(&right.parameter_types))
                .then_with(|| left.return_type.cmp(&right.return_type))
                .then_with(|| left.call_flags.cmp(&right.call_flags))
                .then_with(|| left.dynamic_call.cmp(&right.dynamic_call))
        });
        ApplicationContextProfile {
            context_capacity: MAX_APPLICATION_CONTEXT_PROFILES,
            contexts,
            other_context_loads: self.other_context_loads,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContractState, ExecutionContextState};
    use neo_manifest::{ContractManifest, NefFile};
    use neo_primitives::CallFlags;
    use neo_vm::{ExecutionContext, OpCode, ReferenceCounter, Script};
    use std::sync::Arc;

    fn context_with_discriminator(
        discriminator: u8,
    ) -> ApplicationExecutionContext<neo_storage::EmptyCacheBacking> {
        let raw_script = vec![
            OpCode::PUSHDATA1.byte(),
            1,
            discriminator,
            OpCode::RET.byte(),
        ];
        ExecutionContext::new_with_state(
            Script::new_relaxed(raw_script),
            0,
            &ReferenceCounter::new(),
            ExecutionContextState::default(),
        )
    }

    fn tied_profile_entry(call_flags: u8, dynamic_call: bool) -> ApplicationContextProfileEntry {
        ApplicationContextProfileEntry {
            raw_script_hash: "0x01".to_string(),
            raw_script_bytes: 1,
            entry_offset: 0,
            logical_script_hash: "0x02".to_string(),
            contract_id: Some(1),
            contract_update_counter: Some(2),
            nef_checksum: Some(3),
            manifest_name: Some("contract".to_string()),
            method: Some("method".to_string()),
            argument_count: 1,
            parameter_types: vec!["Integer".to_string()],
            return_type: Some("Boolean".to_string()),
            call_flags,
            dynamic_call,
            context_loads: 1,
        }
    }

    #[test]
    fn collector_maps_raw_bytecode_to_versioned_logical_contract() {
        let raw_script = vec![OpCode::NOP.byte(), OpCode::RET.byte()];
        let mut manifest = ContractManifest::new("ProfiledContract".to_string());
        manifest.abi.methods.push(
            neo_manifest::ContractMethodDescriptor::new(
                "run".to_string(),
                Vec::new(),
                neo_primitives::ContractParameterType::Void,
                1,
                false,
            )
            .expect("valid test method"),
        );
        let contract_hash = UInt160::from([0x42; 20]);
        let mut contract = ContractState::new(
            17,
            contract_hash,
            NefFile::new("profile-test".to_string(), raw_script.clone()),
            manifest,
        );
        contract.update_counter = 3;
        let mut state = ExecutionContextState::<neo_storage::EmptyCacheBacking>::default();
        state.script_hash = Some(contract_hash);
        state.contract = Some(Arc::new(contract.clone()));
        state.argument_count = 2;
        state.call_flags = CallFlags::READ_STATES;
        let mut context = ExecutionContext::new_with_state(
            Script::new_relaxed(raw_script),
            0,
            &ReferenceCounter::new(),
            state,
        );
        context
            .set_instruction_pointer(1)
            .expect("valid entry offset");

        let mut collector = ApplicationContextProfileCollector::default();
        collector.record(&context);
        collector.record(&context);
        let profile = collector.snapshot();

        assert_eq!(profile.contexts.len(), 1);
        let entry = &profile.contexts[0];
        assert_eq!(entry.logical_script_hash, contract_hash.to_string());
        assert_eq!(entry.contract_id, Some(17));
        assert_eq!(entry.contract_update_counter, Some(3));
        assert_eq!(entry.nef_checksum, Some(contract.nef.checksum));
        assert_eq!(entry.manifest_name.as_deref(), Some("ProfiledContract"));
        assert_eq!(entry.method.as_deref(), Some("run"));
        assert_eq!(entry.argument_count, 2);
        assert_eq!(entry.call_flags, CallFlags::READ_STATES.bits());
        assert_eq!(entry.context_loads, 2);
        assert_eq!(profile.other_context_loads, 0);
    }

    #[test]
    fn snapshot_order_is_deterministic_for_complete_invocation_identity() {
        let entries = vec![
            tied_profile_entry(2, true),
            tied_profile_entry(1, true),
            tied_profile_entry(2, false),
            tied_profile_entry(1, false),
        ];
        let forward = ApplicationContextProfileCollector {
            contexts: entries.clone(),
            other_context_loads: 7,
        }
        .snapshot();
        let reverse = ApplicationContextProfileCollector {
            contexts: entries.into_iter().rev().collect(),
            other_context_loads: 7,
        }
        .snapshot();

        assert_eq!(forward, reverse);
        assert_eq!(
            forward
                .contexts
                .iter()
                .map(|entry| (entry.call_flags, entry.dynamic_call))
                .collect::<Vec<_>>(),
            vec![(1, false), (1, true), (2, false), (2, true)]
        );
    }

    #[test]
    fn collector_enforces_application_context_capacity() {
        let mut collector = ApplicationContextProfileCollector::default();
        for discriminator in 0..=MAX_APPLICATION_CONTEXT_PROFILES {
            collector.record(&context_with_discriminator(discriminator as u8));
        }
        collector.record(&context_with_discriminator(0));

        let profile = collector.snapshot();
        assert_eq!(profile.context_capacity, MAX_APPLICATION_CONTEXT_PROFILES);
        assert_eq!(profile.contexts.len(), MAX_APPLICATION_CONTEXT_PROFILES);
        assert_eq!(profile.other_context_loads, 1);
        assert_eq!(
            profile
                .contexts
                .iter()
                .map(|entry| entry.context_loads)
                .sum::<u64>(),
            (MAX_APPLICATION_CONTEXT_PROFILES + 1) as u64
        );
        assert_eq!(
            profile
                .contexts
                .iter()
                .filter(|entry| entry.context_loads == 2)
                .count(),
            1
        );
    }
}
