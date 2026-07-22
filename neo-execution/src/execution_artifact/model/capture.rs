use super::*;
use crate::application_engine::ApplicationEngine;
use crate::diagnostic::Diagnostic;
use crate::execution_artifact::bounds::{ExecutionArtifactError, ExecutionArtifactLimits};
use crate::execution_artifact::observation::{
    CallObservationSnapshotOutcome, ContextObservationSnapshotValue, ExecutionObservationJournal,
};
use crate::execution_artifact::stack::CanonicalGraphBuilder;
use crate::host_access_audit::{ResolvedNativeCacheScope, ResolvedStorageRangeDomain};
use crate::native_contract_provider::NativeContractProvider;
use neo_config::ProtocolSettings;
use neo_primitives::{UInt256, Verifiable};
use neo_storage::{CacheRead, DataCache};
use neo_vm::ContractResolutionIdentity;
use std::sync::Arc;

impl CanonicalExecutionArtifact {
    /// Captures complete final engine state plus non-final observations.
    pub fn capture<P, D, B>(
        engine: &ApplicationEngine<P, D, B>,
        observations: &ExecutionObservationJournal,
        limits: ExecutionArtifactLimits,
    ) -> Result<Self, ExecutionArtifactError>
    where
        P: NativeContractProvider + 'static,
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        Self::check_observation_bounds(observations, limits)?;
        let mut graph = CanonicalGraphBuilder::new(limits);
        let environment = capture_environment(engine, &mut graph)?;
        let result_stack = graph.roots(&engine.result_stack().to_vec())?;
        let fault_exception = graph.optional_root(engine.uncaught_exception_item())?;

        let mut storage_change_count = 0usize;
        let storage_changes = capture_storage_changes(
            engine.snapshot_cache().as_ref(),
            &mut graph,
            &mut storage_change_count,
        )?;
        let invocation_stack =
            capture_invocation_stack(engine, &mut graph, &mut storage_change_count, limits)?;
        let storage_reads = capture_storage_reads(observations, &mut graph)?;
        let storage_ranges = capture_storage_ranges(observations, &mut graph)?;
        let native_cache = capture_native_cache(observations, &mut graph)?;
        let pending_call_count = engine.pending_native_call_count();
        CanonicalGraphBuilder::require_count(
            observations.calls.len().saturating_add(pending_call_count),
            limits.max_calls,
            "calls",
        )?;
        let calls = capture_calls(observations, &mut graph)?;
        let pending_native_calls = capture_pending_calls(engine, &mut graph)?;
        let witnesses = observations
            .witnesses
            .iter()
            .map(|observation| {
                if let WitnessObservationOutcome::Fault(message) = &observation.outcome {
                    graph.retain_bytes(message.len())?;
                }
                Ok(CanonicalWitnessObservation {
                    hash: observation.hash,
                    outcome: observation.outcome.clone(),
                })
            })
            .collect::<Result<Vec<_>, ExecutionArtifactError>>()?;
        let contexts = capture_contexts(observations, &mut graph)?;
        let notifications = capture_notifications(engine, &mut graph)?;
        let logs = capture_logs(engine, &mut graph)?;
        let diagnostics = capture_diagnostics(observations, &mut graph)?;
        let iterators = capture_iterators(engine, &mut graph, limits)?;

        let fault_message = engine.fault_exception().map(str::to_owned);
        if let Some(message) = &fault_message {
            graph.retain_bytes(message.len())?;
        }
        let invocation_counters = engine.invocation_counters_snapshot();
        CanonicalGraphBuilder::require_count(
            invocation_counters.len(),
            limits.max_invocation_counters,
            "invocation counters",
        )?;
        let stack_graph = graph.finish();

        Ok(Self {
            environment,
            vm_state: engine.state(),
            instructions_executed: engine.instructions_executed(),
            gas_consumed_pico: engine.gas_consumed_pico(),
            fee_consumed_pico: engine.fee_consumed_pico(),
            reference_count: engine.reference_count(),
            fault_message,
            fault_exception,
            result_stack,
            invocation_stack,
            invocation_counters,
            storage_changes,
            storage_reads,
            storage_ranges,
            native_cache,
            calls,
            pending_native_calls,
            witnesses,
            contexts,
            fee_charges: observations.fee_charges.clone(),
            notifications,
            logs,
            diagnostics,
            iterators,
            stack_graph,
        })
    }

    fn check_observation_bounds(
        observations: &ExecutionObservationJournal,
        limits: ExecutionArtifactLimits,
    ) -> Result<(), ExecutionArtifactError> {
        CanonicalGraphBuilder::require_count(
            observations.storage_reads.len(),
            limits.max_storage_reads,
            "storage read observations",
        )?;
        CanonicalGraphBuilder::require_count(
            observations.storage_ranges.len(),
            limits.max_storage_ranges,
            "storage range observations",
        )?;
        let storage_range_rows = observations
            .storage_ranges
            .iter()
            .fold(0usize, |rows, observation| {
                rows.saturating_add(observation.rows.len())
            });
        CanonicalGraphBuilder::require_count(
            storage_range_rows,
            limits.max_storage_range_rows,
            "storage range rows",
        )?;
        CanonicalGraphBuilder::require_count(
            observations.native_cache.len(),
            limits.max_native_cache_observations,
            "native-cache observations",
        )?;
        CanonicalGraphBuilder::require_count(observations.calls.len(), limits.max_calls, "calls")?;
        CanonicalGraphBuilder::require_count(
            observations.witnesses.len(),
            limits.max_witnesses,
            "witness observations",
        )?;
        CanonicalGraphBuilder::require_count(
            observations.contexts.len(),
            limits.max_context_observations,
            "context observations",
        )?;
        CanonicalGraphBuilder::require_count(
            observations.fee_charges.len(),
            limits.max_fee_charges,
            "fee-charge observations",
        )?;
        CanonicalGraphBuilder::require_count(
            observations.diagnostics.len(),
            limits.max_diagnostics,
            "diagnostic observations",
        )?;
        Ok(())
    }
}

fn capture_protocol(
    settings: &ProtocolSettings,
    graph: &mut CanonicalGraphBuilder,
) -> Result<ProtocolEnvironmentArtifact, ExecutionArtifactError> {
    let mut standby_committee = Vec::with_capacity(settings.standby_committee.len());
    for point in &settings.standby_committee {
        let bytes = point.to_bytes();
        graph.retain_bytes(bytes.len())?;
        standby_committee.push(bytes);
    }
    let mut hardforks = settings.hardforks.iter().collect::<Vec<_>>();
    hardforks.sort_unstable_by_key(|(hardfork, _)| hardfork.index());
    Ok(ProtocolEnvironmentArtifact {
        network: settings.network,
        address_version: settings.address_version,
        standby_committee,
        validators_count: settings.validators_count,
        milliseconds_per_block: settings.milliseconds_per_block,
        max_valid_until_block_increment: settings.max_valid_until_block_increment,
        max_transactions_per_block: settings.max_transactions_per_block,
        max_block_size: settings.max_block_size,
        max_traceable_blocks: settings.max_traceable_blocks,
        hardforks,
        initial_gas_distribution: settings.initial_gas_distribution,
    })
}

fn container_hash(
    container: Option<&Arc<neo_payloads::VerifiableContainer>>,
) -> Result<Option<UInt256>, ExecutionArtifactError> {
    container
        .map(|container| {
            container
                .hash()
                .map_err(|error| ExecutionArtifactError::InvalidHash {
                    kind: "script container",
                    message: error.to_string(),
                })
        })
        .transpose()
}

fn capture_environment<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    graph: &mut CanonicalGraphBuilder,
) -> Result<ExecutionEnvironmentArtifact, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    let (vm_gas_consumed, vm_gas_limit) = engine.vm_gas_state();
    let (vm_is_jumping, vm_call_flags) = engine.vm_control_state();
    let persisting_block_hash = engine
        .persisting_block()
        .map(|block| {
            block
                .try_hash()
                .map_err(|error| ExecutionArtifactError::InvalidHash {
                    kind: "persisting block",
                    message: error.to_string(),
                })
        })
        .transpose()?;
    Ok(ExecutionEnvironmentArtifact {
        trigger: engine.trigger(),
        protocol: capture_protocol(engine.protocol_settings(), graph)?,
        current_block_index: engine.current_block_index(),
        persisting_block_hash,
        persisting_block_timestamp: engine
            .persisting_block()
            .map(|block| block.header.timestamp()),
        script_container_hash: container_hash(engine.script_container())?,
        current_script_hash: engine.current_script_hash(),
        calling_script_hash: engine.get_calling_script_hash(),
        entry_script_hash: engine.entry_script_hash(),
        call_flags: engine.effective_call_flags().bits(),
        fee_limit_pico: engine.fee_amount_pico(),
        exec_fee_factor: engine.exec_fee_factor_raw(),
        storage_price: engine.storage_price(),
        random_times: engine.random_times(),
        nonce_data: engine.nonce_data(),
        native_calling_override: engine.native_calling_override(),
        native_argument_null_mask: engine.native_argument_null_mask(),
        native_return_is_null: engine.native_return_is_null(),
        next_iterator_id: engine.next_iterator_id(),
        vm_gas_consumed,
        vm_gas_limit,
        vm_is_jumping,
        vm_call_flags,
    })
}

fn capture_storage_changes<B: CacheRead>(
    cache: &DataCache<B>,
    graph: &mut CanonicalGraphBuilder,
    total_count: &mut usize,
) -> Result<Vec<CanonicalStorageChange>, ExecutionArtifactError> {
    let mut changes = Vec::new();
    let mut error = None;
    cache.visit_raw_changes(|key, value| {
        if error.is_some() {
            return;
        }
        *total_count = total_count.saturating_add(1);
        if let Err(limit) = CanonicalGraphBuilder::require_count(
            *total_count,
            graph.limits.max_storage_changes,
            "storage changes",
        ) {
            error = Some(limit);
            return;
        }
        let value_len = value.map_or(0, <[u8]>::len);
        if let Err(limit) = graph.retain_bytes(key.len().saturating_add(value_len)) {
            error = Some(limit);
            return;
        }
        changes.push(CanonicalStorageChange {
            key: key.to_vec(),
            value: value.map(<[u8]>::to_vec),
        });
    });
    if let Some(error) = error {
        return Err(error);
    }
    changes.sort_unstable_by(|left, right| left.key.cmp(&right.key));
    Ok(changes)
}

fn canonical_group<T>(
    values: &[T],
    index: usize,
    shares: impl Fn(&T, &T) -> bool,
) -> Result<u32, ExecutionArtifactError> {
    let first = (0..=index)
        .find(|candidate| shares(&values[index], &values[*candidate]))
        .unwrap_or(index);
    u32::try_from(first).map_err(|_| ExecutionArtifactError::NumericOverflow {
        field: "invocation frame group",
    })
}

fn capture_invocation_stack<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    graph: &mut CanonicalGraphBuilder,
    storage_change_count: &mut usize,
    limits: ExecutionArtifactLimits,
) -> Result<Vec<CanonicalInvocationFrame>, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    let contexts = engine.invocation_stack();
    CanonicalGraphBuilder::require_count(
        contexts.len(),
        limits.max_invocation_frames,
        "invocation frames",
    )?;
    let mut frames = Vec::with_capacity(contexts.len());
    for (index, context) in contexts.iter().enumerate() {
        let script = graph.script_node(&context.script_arc())?;
        let instruction_pointer = u64::try_from(context.instruction_pointer()).map_err(|_| {
            ExecutionArtifactError::NumericOverflow {
                field: "instruction pointer",
            }
        })?;
        let evaluation_stack = graph.roots(&context.evaluation_stack().to_vec())?;
        let static_values =
            context.with_static_fields_mut(|slot| slot.as_ref().map(neo_vm::Slot::to_vec));
        let static_fields = static_values
            .as_deref()
            .map(|values| graph.roots(values))
            .transpose()?;
        let local_variables = context
            .local_variables()
            .map(neo_vm::Slot::to_vec)
            .as_deref()
            .map(|values| graph.roots(values))
            .transpose()?;
        let arguments = context
            .arguments()
            .map(neo_vm::Slot::to_vec)
            .as_deref()
            .map(|values| graph.roots(values))
            .transpose()?;
        let try_stack = context
            .try_stack()
            .into_iter()
            .flatten()
            .map(|frame| CanonicalExceptionFrame {
                catch_pointer: frame.catch_pointer(),
                finally_pointer: frame.finally_pointer(),
                end_pointer: frame.end_pointer(),
                state: frame.state(),
            })
            .collect();

        let state_arc = context.state();
        let state = state_arc.lock();
        let method_name = state.method_name.clone();
        if let Some(method) = &method_name {
            graph.retain_bytes(method.len())?;
        }
        let contract = state.contract.as_ref().map(|contract| {
            ContractResolutionIdentity::new(
                contract.hash,
                contract.id,
                contract.update_counter,
                contract.nef.checksum,
            )
        });
        let snapshot_cache = state.snapshot_cache.clone();
        let calling_context = state.calling_context.clone();
        let application_base = (
            state.script_hash,
            state.calling_script_hash,
            state.native_calling_script_hash,
            contract,
            state.call_flags.bits(),
            state.notification_count,
            state.is_dynamic_call,
            state.whitelisted,
            method_name,
            state.argument_count,
            state.return_type,
            state.parameter_types.clone(),
        );
        drop(state);

        let calling_context = calling_context
            .as_ref()
            .map(|calling| {
                let calling_script = graph.script_node(&calling.script_arc())?;
                let calling_ip = u64::try_from(calling.instruction_pointer()).map_err(|_| {
                    ExecutionArtifactError::NumericOverflow {
                        field: "calling context instruction pointer",
                    }
                })?;
                let calling_state_arc = calling.state();
                let calling_state = calling_state_arc.lock();
                Ok(CallingContextArtifact {
                    script: calling_script,
                    instruction_pointer: calling_ip,
                    script_hash: calling_state.script_hash,
                    calling_script_hash: calling_state.calling_script_hash,
                    native_calling_script_hash: calling_state.native_calling_script_hash,
                    has_calling_context: calling_state.calling_context.is_some(),
                })
            })
            .transpose()?;
        let snapshot_changes = snapshot_cache
            .as_deref()
            .map(|cache| capture_storage_changes(cache, graph, storage_change_count))
            .transpose()?
            .unwrap_or_default();
        let application = ApplicationContextStateArtifact {
            script_hash: application_base.0,
            calling_script_hash: application_base.1,
            calling_context,
            native_calling_script_hash: application_base.2,
            contract: application_base.3,
            call_flags: application_base.4,
            snapshot_changes,
            notification_count: application_base.5,
            is_dynamic_call: application_base.6,
            whitelisted: application_base.7,
            method_name: application_base.8,
            argument_count: application_base.9,
            return_type: application_base.10,
            parameter_types: application_base.11,
        };

        frames.push(CanonicalInvocationFrame {
            script,
            instruction_pointer,
            return_value_count: context.rvcount(),
            evaluation_stack_group: canonical_group(contexts, index, |a, b| {
                a.shares_evaluation_stack_with(b)
            })?,
            static_fields_group: canonical_group(contexts, index, |a, b| {
                a.shares_static_fields_with(b)
            })?,
            state_group: canonical_group(contexts, index, |a, b| a.shares_state_with(b))?,
            native_call_boundary: engine.is_native_call_boundary(context),
            evaluation_stack,
            static_fields,
            local_variables,
            arguments,
            try_stack,
            application,
        });
    }
    Ok(frames)
}

fn capture_storage_reads(
    observations: &ExecutionObservationJournal,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalStoragePointObservation>, ExecutionArtifactError> {
    observations
        .storage_reads
        .iter()
        .map(|observation| {
            graph.retain_bytes(
                std::mem::size_of::<i32>()
                    .saturating_add(observation.key.key().len())
                    .saturating_add(observation.value.as_ref().map_or(0, Vec::len)),
            )?;
            Ok(CanonicalStoragePointObservation {
                key: observation.key.as_bytes().into_owned(),
                value: observation.value.clone(),
            })
        })
        .collect()
}
fn capture_storage_ranges(
    observations: &ExecutionObservationJournal,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalStorageRangeObservation>, ExecutionArtifactError> {
    observations
        .storage_ranges
        .iter()
        .map(|observation| {
            retain_range_access(&observation.access, graph)?;
            CanonicalGraphBuilder::require_count(
                observation.rows.len(),
                observation.access.max_items() as usize,
                "storage range rows",
            )?;
            let mut rows = Vec::with_capacity(observation.rows.len());
            for (key, value) in &observation.rows {
                graph.retain_bytes(
                    std::mem::size_of::<i32>()
                        .saturating_add(key.key().len())
                        .saturating_add(value.len()),
                )?;
                rows.push((key.as_bytes().into_owned(), value.clone()));
            }
            Ok(CanonicalStorageRangeObservation {
                access: observation.access.clone(),
                rows,
            })
        })
        .collect()
}

fn capture_native_cache(
    observations: &ExecutionObservationJournal,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalNativeCacheObservation>, ExecutionArtifactError> {
    observations
        .native_cache
        .iter()
        .map(|observation| {
            if let ResolvedNativeCacheScope::Entry(key) = observation.access.scope() {
                graph.retain_bytes(key.len())?;
            }
            graph.retain_bytes(
                observation
                    .before
                    .as_ref()
                    .map_or(0, Vec::len)
                    .saturating_add(observation.after.as_ref().map_or(0, Vec::len)),
            )?;
            Ok(CanonicalNativeCacheObservation {
                access: observation.access.clone(),
                before: observation.before.clone(),
                after: observation.after.clone(),
            })
        })
        .collect()
}

fn retain_range_access(
    access: &crate::host_access_audit::StorageRangeAccess,
    graph: &mut CanonicalGraphBuilder,
) -> Result<(), ExecutionArtifactError> {
    let bytes = match access.domain() {
        ResolvedStorageRangeDomain::WholeStore => 0,
        ResolvedStorageRangeDomain::Prefix(prefix) => prefix.len(),
        ResolvedStorageRangeDomain::HalfOpen { start, end } => {
            start.len().saturating_add(end.len())
        }
    };
    graph.retain_bytes(bytes)
}

fn capture_calls(
    observations: &ExecutionObservationJournal,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalCallObservation>, ExecutionArtifactError> {
    observations
        .calls
        .iter()
        .map(|observation| {
            graph.retain_bytes(observation.access.method().len())?;
            graph.retain_document(observation.stack.as_ref())?;
            let outcome = match &observation.outcome {
                CallObservationSnapshotOutcome::Returned { value_count } => {
                    CanonicalCallOutcome::Returned {
                        value_count: *value_count,
                    }
                }
                CallObservationSnapshotOutcome::Fault {
                    message,
                    has_exception,
                } => {
                    graph.retain_bytes(message.len())?;
                    CanonicalCallOutcome::Fault {
                        message: message.clone(),
                        has_exception: *has_exception,
                    }
                }
            };
            Ok(CanonicalCallObservation {
                access: observation.access.clone(),
                argument_count: observation.argument_count,
                stack: observation.stack.clone(),
                outcome,
            })
        })
        .collect()
}

fn capture_pending_calls<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalPendingNativeCall>, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    graph.retain_bytes(engine.pending_native_call_method_bytes())?;
    let mut calls = Vec::with_capacity(engine.pending_native_call_count());
    let mut error = None;
    engine.visit_pending_native_calls(|calling_script_hash, contract_hash, method, arguments| {
        if error.is_some() {
            return;
        }
        match graph.roots(arguments) {
            Ok(arguments) => calls.push(CanonicalPendingNativeCall {
                calling_script_hash,
                contract_hash,
                method: method.to_owned(),
                arguments,
            }),
            Err(capture_error) => error = Some(capture_error),
        }
    });
    if let Some(error) = error {
        return Err(error);
    }
    Ok(calls)
}

fn capture_contexts(
    observations: &ExecutionObservationJournal,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalContextObservation>, ExecutionArtifactError> {
    observations
        .contexts
        .iter()
        .map(|observation| {
            let value = match &observation.value {
                ContextObservationSnapshotValue::Boolean(value) => {
                    CanonicalContextObservationValue::Boolean(*value)
                }
                ContextObservationSnapshotValue::U8(value) => {
                    CanonicalContextObservationValue::U8(*value)
                }
                ContextObservationSnapshotValue::U32(value) => {
                    CanonicalContextObservationValue::U32(*value)
                }
                ContextObservationSnapshotValue::U64(value) => {
                    CanonicalContextObservationValue::U64(*value)
                }
                ContextObservationSnapshotValue::I64(value) => {
                    CanonicalContextObservationValue::I64(*value)
                }
                ContextObservationSnapshotValue::Trigger(value) => {
                    CanonicalContextObservationValue::Trigger(*value)
                }
                ContextObservationSnapshotValue::CallFlags(value) => {
                    CanonicalContextObservationValue::CallFlags(*value)
                }
                ContextObservationSnapshotValue::Hash160(value) => {
                    CanonicalContextObservationValue::Hash160(*value)
                }
                ContextObservationSnapshotValue::Hash256(value) => {
                    CanonicalContextObservationValue::Hash256(*value)
                }
                ContextObservationSnapshotValue::StackItems(stack) => {
                    graph.retain_document(stack.as_ref())?;
                    CanonicalContextObservationValue::StackItems(stack.clone())
                }
            };
            Ok(CanonicalContextObservation {
                access: observation.access,
                value,
            })
        })
        .collect()
}

fn capture_notifications<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalNotification>, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    let event_count = engine
        .notifications()
        .len()
        .saturating_add(engine.logs().len());
    CanonicalGraphBuilder::require_count(event_count, graph.limits.max_events, "events")?;
    engine
        .notifications()
        .iter()
        .map(|event| {
            graph.retain_bytes(event.event_name.len())?;
            Ok(CanonicalNotification {
                script_container_hash: container_hash(event.script_container.as_ref())?,
                script_hash: event.script_hash,
                event_name: event.event_name.clone(),
                state: graph.roots(event.state())?,
                state_array: graph
                    .roots(std::slice::from_ref(&event.state_array()))?
                    .remove(0),
            })
        })
        .collect()
}

fn capture_logs<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalLog>, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    engine
        .logs()
        .iter()
        .map(|event| {
            graph.retain_bytes(event.message.len())?;
            Ok(CanonicalLog {
                script_container_hash: container_hash(event.script_container.as_ref())?,
                script_hash: event.script_hash,
                message: event.message.clone(),
            })
        })
        .collect()
}

fn capture_diagnostics(
    observations: &ExecutionObservationJournal,
    graph: &mut CanonicalGraphBuilder,
) -> Result<Vec<CanonicalDiagnosticObservation>, ExecutionArtifactError> {
    observations
        .diagnostics
        .iter()
        .map(|observation| {
            graph.retain_bytes(observation.instruction.len())?;
            graph.retain_document(observation.stack.as_ref())?;
            Ok(CanonicalDiagnosticObservation {
                kind: observation.kind,
                script_hash: observation.script_hash,
                instruction_pointer: observation.instruction_pointer,
                instruction: observation.instruction.clone(),
                stack: observation.stack.clone(),
            })
        })
        .collect()
}

fn capture_iterators<P, D, B>(
    engine: &ApplicationEngine<P, D, B>,
    graph: &mut CanonicalGraphBuilder,
    limits: ExecutionArtifactLimits,
) -> Result<Vec<CanonicalStorageIterator>, ExecutionArtifactError>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    CanonicalGraphBuilder::require_count(
        engine.storage_iterator_count(),
        limits.max_iterators,
        "iterators",
    )?;
    CanonicalGraphBuilder::require_count(
        engine.storage_iterator_row_count(),
        limits.max_iterator_rows,
        "iterator rows",
    )?;
    graph.retain_bytes(engine.storage_iterator_retained_bytes())?;
    let mut iterators = Vec::with_capacity(engine.storage_iterator_count());
    for id in engine.storage_iterator_ids() {
        let iterator = engine
            .get_storage_iterator(id)
            .expect("iterator identifiers are collected from the same immutable engine");
        let mut rows = Vec::with_capacity(iterator.artifact_row_count());
        iterator.visit_artifact_rows(|key, value| {
            rows.push((
                key.as_bytes().into_owned(),
                value.value_bytes().into_owned(),
            ));
        });
        let (current, prefix_length, options_bits) = iterator.artifact_metadata();
        iterators.push(CanonicalStorageIterator {
            id,
            rows,
            current,
            prefix_length,
            options_bits,
        });
    }
    Ok(iterators)
}
