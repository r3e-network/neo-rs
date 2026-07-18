//! Bounded structural validation for candidate declarations.

use super::*;

/// Candidate declaration validation failure.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CandidateContractError {
    /// A configured declaration limit is zero.
    #[error("candidate contract limit `{section}` must be non-zero")]
    ZeroLimit {
        /// Name of the invalid limit.
        section: &'static str,
    },
    /// A declaration exceeds its configured hard bound.
    #[error("candidate contract section `{section}` has {actual} items, maximum {maximum}")]
    LimitExceeded {
        /// Bounded declaration section.
        section: &'static str,
        /// Observed count or byte size.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// Candidate identifier zero is reserved.
    #[error("candidate identifier zero is reserved")]
    ReservedCandidateId,
    /// Candidate version zero is reserved.
    #[error("candidate version zero is reserved")]
    ReservedCandidateVersion,
    /// A deployed-script specialization omitted exact contract update identity.
    #[error("deployed-script specialization requires exact contract resolution identity")]
    MissingContractIdentity,
    /// The entry byte offset is not inside the retained script bytes.
    #[error("candidate entry byte offset is outside the exact script")]
    EntryOutsideScript,
    /// A value constraint is incompatible with its declared stack-item type.
    #[error("stack-item constraint is incompatible with its concrete NeoVM type")]
    InvalidStackItemConstraint,
    /// A value eligibility set contains no supported shape.
    #[error("stack-item eligibility must contain at least one shape")]
    EmptyStackItemEligibility,
    /// A value eligibility set repeats an exact shape.
    #[error("stack-item eligibility contains a duplicate shape")]
    DuplicateStackItemShape,
    /// A normalized argument index is repeated or not in contiguous order.
    #[error("normalized argument declarations must be unique and contiguous from zero")]
    InvalidArgumentOrder,
    /// A VM slot read is declared more than once.
    #[error("duplicate VM slot dependency")]
    DuplicateSlotDependency,
    /// An execution-context input is declared more than once.
    #[error("duplicate execution-context dependency")]
    DuplicateContextDependency,
    /// An execution-context eligibility constraint is self-contradictory.
    #[error("execution-context eligibility constraint is contradictory")]
    InvalidContextConstraint,
    /// A byte expression contains no segments.
    #[error("byte expression must contain at least one segment")]
    EmptyByteExpression,
    /// A byte expression refers to an undeclared argument, slot, or context input.
    #[error("byte expression contains an undeclared input reference")]
    UndeclaredExpressionInput,
    /// A candidate-local dependency, gas-step, or fault ID is duplicated.
    #[error("duplicate identifier {id} in `{section}`")]
    DuplicateIdentifier {
        /// Declaration section.
        section: &'static str,
        /// Repeated candidate-local ID.
        id: u16,
    },
    /// A range declaration permits no rows.
    #[error("range dependency maximum item count must be non-zero")]
    EmptyRangeBound,
    /// A gas declaration contains no ordered charge step.
    #[error("candidate must declare at least one gas step")]
    EmptyGasContract,
    /// A candidate path declares that it executes no VM instructions.
    #[error("candidate instruction count must be non-zero for every supported path")]
    EmptyInstructionCount,
    /// An instruction-count formula references an undeclared argument.
    #[error("instruction count references undeclared argument {argument}")]
    UndeclaredInstructionArgument {
        /// Missing normalized argument index.
        argument: u16,
    },
    /// A gas step references no declared fault.
    #[error("gas step {step} references undeclared fault {fault}")]
    UndeclaredGasFault {
        /// Candidate-local gas-step ID.
        step: u16,
        /// Missing candidate-local fault ID.
        fault: u16,
    },
    /// A gas exhaustion fault is not declared as out-of-gas.
    #[error("gas step {step} exhaustion fault must have the out-of-gas class")]
    InvalidGasFaultClass {
        /// Candidate-local gas-step ID.
        step: u16,
    },
    /// A gas formula references an undeclared normalized argument.
    #[error("gas step {step} references undeclared argument {argument}")]
    UndeclaredGasArgument {
        /// Candidate-local gas-step ID.
        step: u16,
        /// Missing normalized argument index.
        argument: u16,
    },
    /// Stack consumption disagrees with exact normalized argument arity.
    #[error("stack effect consumes {consumed} arguments but eligibility declares {declared}")]
    StackConsumptionMismatch {
        /// Stack-effect consumption count.
        consumed: u16,
        /// Declared normalized argument count.
        declared: u16,
    },
    /// A delete-only storage effect declares a non-zero value bound.
    #[error("delete-only storage effect must declare zero value bytes")]
    DeleteEffectHasValueBytes,
    /// A notification declares an empty event name or zero state bound.
    #[error("notification effect requires a non-empty name and non-zero state-item bound")]
    InvalidNotificationEffect,
    /// A call targets an entry outside its exact target script.
    #[error("call effect entry byte offset is outside the exact target script")]
    CallEntryOutsideScript,
}

pub(super) fn validate_limits(
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    let named = [
        ("arguments", limits.max_arguments),
        ("shapes_per_value", limits.max_shapes_per_value),
        ("slots", limits.max_slots),
        ("context_dependencies", limits.max_context_dependencies),
        ("point_dependencies", limits.max_point_dependencies),
        ("range_dependencies", limits.max_range_dependencies),
        ("native_dependencies", limits.max_native_dependencies),
        ("gas_steps", limits.max_gas_steps),
        ("faults", limits.max_faults),
        ("host_effects", limits.max_host_effects),
        ("expression_segments", limits.max_expression_segments),
        (
            "expression_literal_bytes",
            limits.max_expression_literal_bytes,
        ),
    ];
    for (section, limit) in named {
        if limit == 0 {
            return Err(CandidateContractError::ZeroLimit { section });
        }
    }
    Ok(())
}

pub(super) fn validate_parts(
    parts: &CandidateContractParts,
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    validate_identity(&parts.identity)?;
    validate_count(
        "arguments",
        parts.eligibility.arguments.len(),
        limits.max_arguments,
    )?;
    validate_count("slots", parts.eligibility.slots.len(), limits.max_slots)?;
    validate_count(
        "context_dependencies",
        parts.eligibility.context.len(),
        limits.max_context_dependencies,
    )?;
    validate_count(
        "point_dependencies",
        parts.state.point_reads.len(),
        limits.max_point_dependencies,
    )?;
    validate_count(
        "range_dependencies",
        parts.state.range_reads.len(),
        limits.max_range_dependencies,
    )?;
    validate_count(
        "native_dependencies",
        parts.state.native_reads.len(),
        limits.max_native_dependencies,
    )?;
    validate_count("gas_steps", parts.gas_steps.len(), limits.max_gas_steps)?;
    validate_count("faults", parts.faults.len(), limits.max_faults)?;
    validate_count(
        "host_effects",
        parts.effects.host.len(),
        limits.max_host_effects,
    )?;

    validate_eligibility(&parts.eligibility, limits)?;
    validate_state(&parts.state, &parts.eligibility, limits)?;
    validate_gas_and_faults(parts)?;
    validate_effects(parts, limits)
}

fn validate_identity(identity: &CandidateIdentity) -> Result<(), CandidateContractError> {
    if identity.candidate_id.value() == 0 {
        return Err(CandidateContractError::ReservedCandidateId);
    }
    if identity.candidate_version.value() == 0 {
        return Err(CandidateContractError::ReservedCandidateVersion);
    }
    if identity.execution.contract().is_none() {
        return Err(CandidateContractError::MissingContractIdentity);
    }
    let entry = usize::try_from(identity.execution.entry_ip())
        .map_err(|_| CandidateContractError::EntryOutsideScript)?;
    if entry >= identity.execution.script_len() {
        return Err(CandidateContractError::EntryOutsideScript);
    }
    Ok(())
}

fn validate_eligibility(
    eligibility: &InvocationEligibility,
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    for (expected, argument) in eligibility.arguments.iter().enumerate() {
        if usize::from(argument.index) != expected {
            return Err(CandidateContractError::InvalidArgumentOrder);
        }
        validate_value(&argument.value, limits)?;
    }

    let mut slots = HashSet::with_capacity(eligibility.slots.len());
    for slot in eligibility.slots.iter() {
        validate_value(&slot.value, limits)?;
        if !slots.insert((slot.source, slot.index)) {
            return Err(CandidateContractError::DuplicateSlotDependency);
        }
    }

    let mut context = HashSet::with_capacity(eligibility.context.len());
    for dependency in eligibility.context.iter() {
        if !context.insert(context_domain(dependency)) {
            return Err(CandidateContractError::DuplicateContextDependency);
        }
        if let ContextDependency::CallFlags {
            required,
            forbidden,
        } = dependency
            && required & forbidden != 0
        {
            return Err(CandidateContractError::InvalidContextConstraint);
        }
    }
    Ok(())
}

fn context_domain(dependency: &ContextDependency) -> u8 {
    match dependency {
        ContextDependency::ScriptContainer => 0,
        ContextDependency::PersistingBlock => 1,
        ContextDependency::GasRemaining => 2,
        ContextDependency::CallFlags { .. } => 3,
        ContextDependency::InvocationCounter => 4,
        ContextDependency::EntryScriptHash => 5,
        ContextDependency::CallingScriptHash { .. } => 6,
        ContextDependency::ExecutingScriptHash => 7,
        ContextDependency::RuntimeTime => 8,
        ContextDependency::RuntimeRandom => 9,
        ContextDependency::Diagnostics => 10,
        ContextDependency::FeeWhitelist { .. } => 11,
        ContextDependency::InternalCallFrame => 12,
    }
}

fn validate_value(
    value: &StackItemEligibility,
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    validate_count(
        "shapes_per_value",
        value.accepted.len(),
        limits.max_shapes_per_value,
    )?;
    for shape in value.accepted.iter() {
        shape.validate()?;
    }
    Ok(())
}

fn validate_state(
    state: &StateDependencyContract,
    eligibility: &InvocationEligibility,
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    validate_unique_ids(
        "point_dependencies",
        state.point_reads.iter().map(|dependency| dependency.id),
    )?;
    validate_unique_ids(
        "range_dependencies",
        state.range_reads.iter().map(|dependency| dependency.id),
    )?;
    validate_unique_ids(
        "native_dependencies",
        state.native_reads.iter().map(|dependency| dependency.id),
    )?;

    for dependency in state.point_reads.iter() {
        validate_expression(&dependency.key, eligibility, limits)?;
    }
    for dependency in state.range_reads.iter() {
        if dependency.max_items == 0 {
            return Err(CandidateContractError::EmptyRangeBound);
        }
        match &dependency.domain {
            RangeDomain::Prefix(prefix) => validate_expression(prefix, eligibility, limits)?,
            RangeDomain::HalfOpen { start, end } => {
                validate_expression(start, eligibility, limits)?;
                validate_expression(end, eligibility, limits)?;
            }
        }
    }
    for dependency in state.native_reads.iter() {
        if let NativeCacheScope::Entry(key) = &dependency.scope {
            validate_expression(key, eligibility, limits)?;
        }
    }
    Ok(())
}

fn validate_gas_and_faults(parts: &CandidateContractParts) -> Result<(), CandidateContractError> {
    match parts.instruction_count {
        InstructionCount::Fixed(0)
        | InstructionCount::Decision { when_true: 0, .. }
        | InstructionCount::Decision { when_false: 0, .. }
        | InstructionCount::ArgumentBytes {
            base: 0,
            per_byte: 0,
            ..
        } => return Err(CandidateContractError::EmptyInstructionCount),
        InstructionCount::ArgumentBytes { argument, .. }
            if usize::from(argument) >= parts.eligibility.arguments.len() =>
        {
            return Err(CandidateContractError::UndeclaredInstructionArgument { argument });
        }
        _ => {}
    }

    if parts.gas_steps.is_empty() {
        return Err(CandidateContractError::EmptyGasContract);
    }
    validate_unique_ids("gas_steps", parts.gas_steps.iter().map(|step| step.id))?;
    validate_unique_ids("faults", parts.faults.iter().map(|fault| fault.id))?;

    for step in parts.gas_steps.iter() {
        let Some(fault) = parts
            .faults
            .iter()
            .find(|fault| fault.id == step.exhaustion_fault)
        else {
            return Err(CandidateContractError::UndeclaredGasFault {
                step: step.id,
                fault: step.exhaustion_fault,
            });
        };
        if fault.class != FaultClass::OutOfGas {
            return Err(CandidateContractError::InvalidGasFaultClass { step: step.id });
        }
        if let GasAmount::ArgumentBytes { argument, .. } = step.amount
            && usize::from(argument) >= parts.eligibility.arguments.len()
        {
            return Err(CandidateContractError::UndeclaredGasArgument {
                step: step.id,
                argument,
            });
        }
    }
    Ok(())
}

fn validate_effects(
    parts: &CandidateContractParts,
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    let declared = u16::try_from(parts.eligibility.arguments.len()).unwrap_or(u16::MAX);
    let consumed = parts.effects.stack.consumed_arguments;
    if consumed != declared {
        return Err(CandidateContractError::StackConsumptionMismatch { consumed, declared });
    }
    for result in parts.effects.stack.results.iter() {
        validate_value(result, limits)?;
    }

    for effect in parts.effects.host.iter() {
        match effect {
            HostEffectContract::StorageWrite {
                key,
                kind,
                max_value_bytes,
                ..
            } => {
                validate_expression(key, &parts.eligibility, limits)?;
                if *kind == StorageWriteKind::Delete && *max_value_bytes != 0 {
                    return Err(CandidateContractError::DeleteEffectHasValueBytes);
                }
            }
            HostEffectContract::NativeCacheWrite { scope, .. } => {
                if let NativeCacheScope::Entry(key) = scope {
                    validate_expression(key, &parts.eligibility, limits)?;
                }
            }
            HostEffectContract::ContractCall(call) => validate_call(call, &parts.identity)?,
            HostEffectContract::Notification {
                event_name,
                max_state_items,
                ..
            } => {
                if event_name.is_empty()
                    || std::str::from_utf8(event_name).is_err()
                    || *max_state_items == 0
                {
                    return Err(CandidateContractError::InvalidNotificationEffect);
                }
            }
            HostEffectContract::Log { .. } => {}
            HostEffectContract::WitnessCheck { account } => {
                validate_expression(account, &parts.eligibility, limits)?;
            }
            HostEffectContract::SlotWrite { value, .. } => validate_value(value, limits)?,
        }
    }
    Ok(())
}

fn validate_call(
    call: &CallContract,
    identity: &CandidateIdentity,
) -> Result<(), CandidateContractError> {
    let target = match call.target {
        ContractTarget::ExecutingContract => identity.execution(),
        ContractTarget::Exact(_) => return Ok(()),
    };
    if usize::try_from(call.entry_ip).map_or(true, |entry| entry >= target.script_len()) {
        return Err(CandidateContractError::CallEntryOutsideScript);
    }
    Ok(())
}

fn validate_expression(
    expression: &ByteExpression,
    eligibility: &InvocationEligibility,
    limits: CandidateContractLimits,
) -> Result<(), CandidateContractError> {
    validate_count(
        "expression_segments",
        expression.segments.len(),
        limits.max_expression_segments,
    )?;
    validate_count(
        "expression_literal_bytes",
        expression.literal_bytes(),
        limits.max_expression_literal_bytes,
    )?;
    for segment in expression.segments.iter() {
        let declared = match segment {
            ByteExpressionSegment::Literal(_) => true,
            ByteExpressionSegment::Argument(index) => {
                usize::from(*index) < eligibility.arguments.len()
            }
            ByteExpressionSegment::Slot { source, index } => eligibility
                .slots
                .iter()
                .any(|slot| slot.source == *source && slot.index == *index),
            ByteExpressionSegment::ScriptHash(ContextScriptHash::Entry) => eligibility
                .context
                .contains(&ContextDependency::EntryScriptHash),
            ByteExpressionSegment::ScriptHash(ContextScriptHash::Calling) => {
                eligibility.context.iter().any(|dependency| {
                    matches!(dependency, ContextDependency::CallingScriptHash { .. })
                })
            }
            ByteExpressionSegment::ScriptHash(ContextScriptHash::Executing) => eligibility
                .context
                .contains(&ContextDependency::ExecutingScriptHash),
        };
        if !declared {
            return Err(CandidateContractError::UndeclaredExpressionInput);
        }
    }
    Ok(())
}

fn validate_count(
    section: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), CandidateContractError> {
    if actual > maximum {
        Err(CandidateContractError::LimitExceeded {
            section,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn validate_unique_ids(
    section: &'static str,
    ids: impl IntoIterator<Item = u16>,
) -> Result<(), CandidateContractError> {
    let mut unique = HashSet::new();
    for id in ids {
        if !unique.insert(id) {
            return Err(CandidateContractError::DuplicateIdentifier { section, id });
        }
    }
    Ok(())
}

pub(super) fn accounted_bytes(parts: &CandidateContractParts) -> usize {
    let mut bytes = size_of::<CandidateContractParts>()
        .saturating_add(parts.identity.execution.script_len())
        .saturating_add(
            size_of::<ArgumentContract>().saturating_mul(parts.eligibility.arguments.len()),
        )
        .saturating_add(size_of::<SlotContract>().saturating_mul(parts.eligibility.slots.len()))
        .saturating_add(
            size_of::<ContextDependency>().saturating_mul(parts.eligibility.context.len()),
        )
        .saturating_add(
            size_of::<PointStateDependency>().saturating_mul(parts.state.point_reads.len()),
        )
        .saturating_add(
            size_of::<RangeStateDependency>().saturating_mul(parts.state.range_reads.len()),
        )
        .saturating_add(
            size_of::<NativeCacheDependency>().saturating_mul(parts.state.native_reads.len()),
        )
        .saturating_add(size_of::<GasStepContract>().saturating_mul(parts.gas_steps.len()))
        .saturating_add(size_of::<FaultContract>().saturating_mul(parts.faults.len()))
        .saturating_add(size_of::<HostEffectContract>().saturating_mul(parts.effects.host.len()));

    for argument in parts.eligibility.arguments.iter() {
        bytes = bytes.saturating_add(argument.value.dynamic_bytes());
    }
    for slot in parts.eligibility.slots.iter() {
        bytes = bytes.saturating_add(slot.value.dynamic_bytes());
    }
    for result in parts.effects.stack.results.iter() {
        bytes = bytes.saturating_add(result.dynamic_bytes());
    }
    for expression in all_expressions(parts) {
        bytes = bytes.saturating_add(expression.accounted_bytes());
    }
    for effect in parts.effects.host.iter() {
        if let HostEffectContract::Notification { event_name, .. } = effect {
            bytes = bytes.saturating_add(event_name.len());
        }
        if let HostEffectContract::SlotWrite { value, .. } = effect {
            bytes = bytes.saturating_add(value.dynamic_bytes());
        }
    }
    bytes
}

fn all_expressions(parts: &CandidateContractParts) -> Vec<&ByteExpression> {
    let mut expressions = Vec::new();
    expressions.extend(parts.state.point_reads.iter().map(|item| &item.key));
    for item in parts.state.range_reads.iter() {
        match &item.domain {
            RangeDomain::Prefix(prefix) => expressions.push(prefix),
            RangeDomain::HalfOpen { start, end } => {
                expressions.push(start);
                expressions.push(end);
            }
        }
    }
    expressions.extend(
        parts
            .state
            .native_reads
            .iter()
            .filter_map(|item| match &item.scope {
                NativeCacheScope::Entry(key) => Some(key),
                NativeCacheScope::WholeDomain => None,
            }),
    );
    for effect in parts.effects.host.iter() {
        match effect {
            HostEffectContract::StorageWrite { key, .. }
            | HostEffectContract::WitnessCheck { account: key } => expressions.push(key),
            HostEffectContract::NativeCacheWrite {
                scope: NativeCacheScope::Entry(key),
                ..
            } => expressions.push(key),
            _ => {}
        }
    }
    expressions
}
