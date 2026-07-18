use super::*;
use crate::{
    ContractResolutionIdentity, ExecutionPlanKey, HardforkPlanState, HardforkTableIdentity,
    ProtocolIdentity, ProtocolVersion, StackItem,
};
use neo_primitives::constants::MAINNET_MAGIC;
use neo_primitives::{Hardfork, TriggerType, UInt160};
use std::sync::Arc;

const CONTRACT_ID: i32 = 27;

fn deployed_contract(byte: u8) -> ContractResolutionIdentity {
    ContractResolutionIdentity::new(UInt160::from([byte; 20]), CONTRACT_ID, 1, 0xB092_1500)
}

fn hardforks() -> HardforkTableIdentity {
    HardforkTableIdentity::unconfigured()
        .with_state(
            Hardfork::HfAspidochelone,
            HardforkPlanState::Active {
                activation_height: 1_730_000,
            },
        )
        .with_state(
            Hardfork::HfBasilisk,
            HardforkPlanState::Pending {
                activation_height: 4_120_000,
            },
        )
}

fn execution_key(
    bytes: &[u8],
    entry_ip: u32,
    protocol: ProtocolIdentity,
    forks: HardforkTableIdentity,
    trigger: TriggerType,
    contract: Option<ContractResolutionIdentity>,
) -> ExecutionPlanKey {
    ExecutionPlanKey::new(
        Arc::<[u8]>::from(bytes),
        entry_ip,
        protocol,
        forks,
        trigger,
        contract,
    )
}

fn base_execution() -> ExecutionPlanKey {
    execution_key(
        &[0x10, 0x11, 0x12, 0x40],
        1,
        ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1),
        hardforks(),
        TriggerType::APPLICATION,
        Some(deployed_contract(0xCA)),
    )
}

fn shape(item_type: crate::StackItemType, constraint: StackItemConstraint) -> StackItemShape {
    StackItemShape::new(item_type, constraint).expect("test shape is valid")
}

fn eligibility(shapes: Vec<StackItemShape>) -> StackItemEligibility {
    StackItemEligibility::new(shapes).expect("test eligibility is valid")
}

fn bytes_20() -> StackItemEligibility {
    eligibility(vec![
        shape(
            crate::StackItemType::ByteString,
            StackItemConstraint::ByteLength { min: 20, max: 20 },
        ),
        shape(
            crate::StackItemType::Buffer,
            StackItemConstraint::ByteLength { min: 20, max: 20 },
        ),
    ])
}

fn output_buffer() -> StackItemEligibility {
    eligibility(vec![shape(
        crate::StackItemType::Buffer,
        StackItemConstraint::ByteLength { min: 41, max: 41 },
    )])
}

fn base_parts(
    candidate_id: u32,
    candidate_version: u16,
    execution: ExecutionPlanKey,
    authority: CandidateAuthority,
) -> CandidateContractParts {
    let arguments = vec![
        ArgumentContract::new(0, bytes_20()),
        ArgumentContract::new(1, bytes_20()),
    ];
    let prefix = eligibility(vec![shape(
        crate::StackItemType::Buffer,
        StackItemConstraint::ExactBytes(Arc::from([0xFF])),
    )]);
    CandidateContractParts {
        identity: CandidateIdentity::new(
            CandidateId::new(candidate_id),
            CandidateVersion::new(candidate_version),
            execution,
        ),
        authority,
        eligibility: InvocationEligibility::new(
            arguments,
            vec![SlotContract::new(SlotSource::Static, 1, prefix)],
            vec![ContextDependency::GasRemaining],
        ),
        state: StateDependencyContract::new(
            Vec::<PointStateDependency>::new(),
            Vec::<RangeStateDependency>::new(),
            Vec::<NativeCacheDependency>::new(),
        ),
        instruction_count: InstructionCount::Decision {
            decision: 0,
            when_true: 19,
            when_false: 18,
        },
        gas_steps: Arc::from([GasStepContract {
            id: 0,
            amount: GasAmount::Decision {
                decision: 0,
                when_true: 24_680,
                when_false: 24_678,
            },
            exhaustion_fault: 0,
        }]),
        faults: Arc::from([FaultContract {
            id: 0,
            class: FaultClass::OutOfGas,
            effects: FaultEffectDisposition::Discard,
        }]),
        effects: EffectContract::new(
            StackEffectContract::new(2, 3, vec![output_buffer()]),
            Vec::<HostEffectContract>::new(),
        ),
    }
}

fn base_candidate() -> CandidateContract {
    CandidateContract::try_new(
        base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly),
        CandidateContractLimits::DEFAULT,
    )
    .expect("base candidate is valid")
}

fn valid_arguments() -> Vec<StackItem> {
    vec![
        StackItem::from_byte_string(vec![0x11; 20]),
        StackItem::from_buffer(vec![0x22; 20]),
    ]
}

#[test]
fn stack_item_shapes_match_only_declared_neo_vm_values() {
    let exact_buffer = shape(
        crate::StackItemType::Buffer,
        StackItemConstraint::ExactBytes(Arc::from([1, 2, 3])),
    );
    assert!(exact_buffer.matches(&StackItem::from_buffer(vec![1, 2, 3])));
    assert!(!exact_buffer.matches(&StackItem::from_buffer(vec![1, 2])));
    assert!(!exact_buffer.matches(&StackItem::from_byte_string(vec![1, 2, 3])));

    let integer = shape(
        crate::StackItemType::Integer,
        StackItemConstraint::SignedIntegerRange { min: -2, max: 3 },
    );
    assert!(integer.matches(&StackItem::from_i64(-2)));
    assert!(integer.matches(&StackItem::from_i64(3)));
    assert!(!integer.matches(&StackItem::from_i64(4)));

    let collection = shape(
        crate::StackItemType::Array,
        StackItemConstraint::CollectionLength { min: 1, max: 2 },
    );
    assert!(collection.matches(&StackItem::from_array(vec![StackItem::Null])));
    assert!(!collection.matches(&StackItem::from_array(Vec::new())));

    assert_eq!(
        StackItemShape::new(
            crate::StackItemType::Integer,
            StackItemConstraint::ExactBytes(Arc::from([1])),
        ),
        Err(CandidateContractError::InvalidStackItemConstraint)
    );
    assert_eq!(
        StackItemShape::new(
            crate::StackItemType::Buffer,
            StackItemConstraint::ByteLength { min: 2, max: 1 },
        ),
        Err(CandidateContractError::InvalidStackItemConstraint)
    );
}

#[test]
fn candidate_retains_exact_identity_and_complete_pure_effect_contract() {
    let candidate = base_candidate();
    let identity = candidate.identity();
    let execution = identity.execution();

    assert_eq!(identity.schema(), SpecializationContractVersion::V1);
    assert_eq!(identity.schema().value(), 1);
    assert_eq!(identity.candidate_id().value(), 1);
    assert_eq!(identity.candidate_version().value(), 1);
    assert_eq!(execution.script_bytes(), &[0x10, 0x11, 0x12, 0x40]);
    assert_eq!(execution.entry_ip(), 1);
    assert_eq!(execution.protocol().network_magic(), MAINNET_MAGIC);
    assert_eq!(
        execution.protocol().version(),
        ProtocolVersion::NEO_N3_V3_10_1
    );
    assert_eq!(execution.hardforks(), hardforks());
    assert_eq!(execution.trigger(), TriggerType::APPLICATION);
    assert_eq!(execution.contract(), Some(deployed_contract(0xCA)));

    assert_eq!(candidate.eligibility().arguments().len(), 2);
    assert_eq!(candidate.eligibility().slots().len(), 1);
    assert_eq!(
        candidate.eligibility().context(),
        &[ContextDependency::GasRemaining]
    );
    assert!(candidate.state().point_reads().is_empty());
    assert!(candidate.state().range_reads().is_empty());
    assert!(candidate.state().native_reads().is_empty());
    assert_eq!(
        candidate.instruction_count(),
        InstructionCount::Decision {
            decision: 0,
            when_true: 19,
            when_false: 18,
        }
    );
    assert_eq!(candidate.gas_steps().len(), 1);
    assert_eq!(candidate.faults()[0].class, FaultClass::OutOfGas);
    assert_eq!(
        candidate.faults()[0].effects,
        FaultEffectDisposition::Discard
    );
    assert_eq!(candidate.effects().stack().consumed_arguments(), 2);
    assert_eq!(candidate.effects().stack().peak_reference_count_delta(), 3);
    assert_eq!(candidate.effects().stack().results().len(), 1);
    assert!(candidate.effects().host().is_empty());
    assert!(candidate.accounted_bytes() >= execution.script_len());
}

#[test]
fn registry_lookup_uses_full_exact_execution_identity() {
    let candidate = base_candidate();
    let registry =
        SpecializationRegistry::try_new([candidate.clone()], SpecializationRegistryLimits::DEFAULT)
            .expect("registry is valid");
    let baseline = candidate.identity().execution();
    assert_eq!(registry.lookup_exact(baseline), Some(&candidate));

    let distinct = [
        execution_key(
            &[0x10, 0x11, 0x13, 0x40],
            1,
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            baseline.contract(),
        ),
        execution_key(
            baseline.script_bytes(),
            2,
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            baseline.contract(),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            ProtocolIdentity::new(0x1234_5678, ProtocolVersion::NEO_N3_V3_10_1),
            baseline.hardforks(),
            baseline.trigger(),
            baseline.contract(),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::new(3, 10, 2)),
            baseline.hardforks(),
            baseline.trigger(),
            baseline.contract(),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            baseline.protocol(),
            baseline.hardforks().with_state(
                Hardfork::HfBasilisk,
                HardforkPlanState::Active {
                    activation_height: 4_120_000,
                },
            ),
            baseline.trigger(),
            baseline.contract(),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            baseline.protocol(),
            baseline.hardforks(),
            TriggerType::VERIFICATION,
            baseline.contract(),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            Some(ContractResolutionIdentity::new(
                UInt160::from([0xCB; 20]),
                CONTRACT_ID,
                1,
                0xB092_1500,
            )),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            Some(ContractResolutionIdentity::new(
                UInt160::from([0xCA; 20]),
                CONTRACT_ID + 1,
                1,
                0xB092_1500,
            )),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            Some(ContractResolutionIdentity::new(
                UInt160::from([0xCA; 20]),
                CONTRACT_ID,
                2,
                0xB092_1500,
            )),
        ),
        execution_key(
            baseline.script_bytes(),
            1,
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            Some(ContractResolutionIdentity::new(
                UInt160::from([0xCA; 20]),
                CONTRACT_ID,
                1,
                0xB092_1501,
            )),
        ),
    ];

    for key in distinct {
        assert!(registry.lookup_exact(&key).is_none(), "accepted {key:?}");
    }
}

#[test]
fn routing_is_disabled_by_default_and_shadow_authority_is_fail_closed() {
    let candidate = base_candidate();
    let execution = candidate.identity().execution().clone();
    let registry =
        SpecializationRegistry::try_new([candidate.clone()], SpecializationRegistryLimits::DEFAULT)
            .expect("registry is valid");
    let arguments = valid_arguments();

    assert_eq!(SpecializationMode::default(), SpecializationMode::Disabled);
    assert_eq!(
        registry.select(&execution, &arguments, SpecializationMode::default()),
        SpecializationSelection::Disabled
    );
    assert_eq!(
        registry.select(&execution, &arguments, SpecializationMode::Shadow),
        SpecializationSelection::Selected {
            candidate: &candidate,
            mode: SpecializationMode::Shadow,
        }
    );
    assert_eq!(
        registry.select(&execution, &arguments, SpecializationMode::Authoritative,),
        SpecializationSelection::AuthorityNotPermitted
    );

    let promoted = CandidateContract::try_new(
        base_parts(
            2,
            1,
            execution.clone(),
            CandidateAuthority::OptInAuthoritative,
        ),
        CandidateContractLimits::DEFAULT,
    )
    .expect("promoted candidate is valid");
    let promoted_registry =
        SpecializationRegistry::try_new([promoted.clone()], SpecializationRegistryLimits::DEFAULT)
            .expect("registry is valid");
    assert_eq!(
        promoted_registry.select(&execution, &arguments, SpecializationMode::Authoritative,),
        SpecializationSelection::Selected {
            candidate: &promoted,
            mode: SpecializationMode::Authoritative,
        }
    );
}

#[test]
fn argument_eligibility_is_exact_and_rechecked_for_each_invocation() {
    let exact = eligibility(vec![shape(
        crate::StackItemType::Buffer,
        StackItemConstraint::ExactBytes(Arc::from([1])),
    )]);
    let buffer = StackItem::from_buffer(vec![1]);
    assert!(exact.matches(&buffer));
    let StackItem::Buffer(value) = &buffer else {
        panic!("constructed a buffer")
    };
    value.set(0, 2).expect("index exists");
    assert!(!exact.matches(&buffer));

    let candidate = base_candidate();
    let execution = candidate.identity().execution().clone();
    let registry =
        SpecializationRegistry::try_new([candidate], SpecializationRegistryLimits::DEFAULT)
            .expect("registry is valid");
    assert_eq!(
        registry.select(
            &execution,
            &[StackItem::from_byte_string(vec![0; 20])],
            SpecializationMode::Shadow,
        ),
        SpecializationSelection::IneligibleArguments
    );
    assert_eq!(
        registry.select(
            &execution,
            &[
                StackItem::from_buffer(vec![0; 19]),
                StackItem::from_buffer(vec![0; 20]),
            ],
            SpecializationMode::Shadow,
        ),
        SpecializationSelection::IneligibleArguments
    );
}

#[test]
fn state_range_native_gas_fault_and_effect_declarations_are_exact() {
    let mut parts = base_parts(3, 2, base_execution(), CandidateAuthority::ShadowOnly);
    parts.eligibility = InvocationEligibility::new(
        parts.eligibility.arguments().to_vec(),
        parts.eligibility.slots().to_vec(),
        vec![
            ContextDependency::GasRemaining,
            ContextDependency::EntryScriptHash,
            ContextDependency::CallingScriptHash {
                expected: Some(UInt160::from([0x44; 20])),
            },
            ContextDependency::ExecutingScriptHash,
        ],
    );
    let argument_key = ByteExpression::new(vec![
        ByteExpressionSegment::Literal(Arc::from([0xAA, 0xBB])),
        ByteExpressionSegment::Argument(0),
    ])
    .expect("expression is non-empty");
    let prefix = ByteExpression::literal(Arc::<[u8]>::from([0xCC]));
    let native_domain = NativeCacheDomain {
        contract_hash: UInt160::from([0x55; 20]),
        contract_id: -5,
        native_version: 7,
        partition: 2,
    };
    parts.state = StateDependencyContract::new(
        vec![PointStateDependency {
            id: 10,
            target: StorageTarget::ExecutingContract,
            key: argument_key.clone(),
            requirement: ReadRequirement::PresentOrAbsent,
        }],
        vec![RangeStateDependency {
            id: 11,
            target: StorageTarget::ExactContract(deployed_contract(0x77)),
            domain: RangeDomain::Prefix(prefix.clone()),
            direction: RangeDirection::Reverse,
            max_items: 128,
        }],
        vec![NativeCacheDependency {
            id: 12,
            domain: native_domain,
            scope: NativeCacheScope::Entry(
                ByteExpression::new(vec![ByteExpressionSegment::ScriptHash(
                    ContextScriptHash::Entry,
                )])
                .expect("expression is non-empty"),
            ),
        }],
    );
    parts.effects = EffectContract::new(
        StackEffectContract::new(2, 3, vec![output_buffer()]),
        vec![
            HostEffectContract::StorageWrite {
                target: StorageTarget::ExecutingContract,
                key: argument_key.clone(),
                kind: StorageWriteKind::PutOrDelete,
                max_value_bytes: 64,
            },
            HostEffectContract::NativeCacheWrite {
                domain: native_domain,
                scope: NativeCacheScope::WholeDomain,
            },
            HostEffectContract::ContractCall(CallContract {
                target: ContractTarget::ExecutingContract,
                entry_ip: 0,
                call_flags: 0x0F,
                argument_count: 2,
                result_count: 1,
            }),
            HostEffectContract::Notification {
                emitter: ContractTarget::ExecutingContract,
                event_name: Arc::from(*b"Transfer"),
                max_state_items: 4,
            },
            HostEffectContract::Log {
                emitter: ContractTarget::Exact(deployed_contract(0x77)),
                max_message_bytes: 1_024,
            },
            HostEffectContract::WitnessCheck {
                account: ByteExpression::new(vec![ByteExpressionSegment::ScriptHash(
                    ContextScriptHash::Calling,
                )])
                .expect("expression is non-empty"),
            },
            HostEffectContract::SlotWrite {
                source: SlotSource::Static,
                index: 1,
                value: output_buffer(),
            },
        ],
    );
    let candidate = CandidateContract::try_new(parts, CandidateContractLimits::DEFAULT)
        .expect("full declaration is valid");

    assert_eq!(candidate.state().point_reads()[0].key, argument_key);
    assert_eq!(candidate.state().range_reads()[0].max_items, 128);
    assert_eq!(candidate.state().native_reads()[0].domain, native_domain);
    assert_eq!(candidate.effects().host().len(), 7);
    assert!(candidate.accounted_bytes() > base_candidate().accounted_bytes());
}

#[test]
fn candidate_validation_rejects_incomplete_identity_and_invocation_shape() {
    let limits = CandidateContractLimits::DEFAULT;

    let mut parts = base_parts(0, 1, base_execution(), CandidateAuthority::ShadowOnly);
    assert_eq!(
        CandidateContract::try_new(parts.clone(), limits),
        Err(CandidateContractError::ReservedCandidateId)
    );
    parts.identity = CandidateIdentity::new(
        CandidateId::new(1),
        CandidateVersion::new(0),
        base_execution(),
    );
    assert_eq!(
        CandidateContract::try_new(parts.clone(), limits),
        Err(CandidateContractError::ReservedCandidateVersion)
    );
    parts.identity = CandidateIdentity::new(
        CandidateId::new(1),
        CandidateVersion::new(1),
        execution_key(
            &[0x40],
            0,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1),
            hardforks(),
            TriggerType::APPLICATION,
            None,
        ),
    );
    assert_eq!(
        CandidateContract::try_new(parts.clone(), limits),
        Err(CandidateContractError::MissingContractIdentity)
    );
    parts.identity = CandidateIdentity::new(
        CandidateId::new(1),
        CandidateVersion::new(1),
        execution_key(
            &[0x40],
            1,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1),
            hardforks(),
            TriggerType::APPLICATION,
            Some(deployed_contract(0xCA)),
        ),
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::EntryOutsideScript)
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.eligibility = InvocationEligibility::new(
        vec![
            ArgumentContract::new(1, bytes_20()),
            ArgumentContract::new(0, bytes_20()),
        ],
        parts.eligibility.slots().to_vec(),
        parts.eligibility.context().to_vec(),
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::InvalidArgumentOrder)
    );
}

#[test]
fn candidate_validation_rejects_undeclared_inputs_and_inconsistent_gas_effects() {
    let limits = CandidateContractLimits::DEFAULT;
    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.instruction_count = InstructionCount::Decision {
        decision: 0,
        when_true: 19,
        when_false: 0,
    };
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::EmptyInstructionCount)
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.instruction_count = InstructionCount::ArgumentBytes {
        argument: 9,
        base: 1,
        per_byte: 1,
    };
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::UndeclaredInstructionArgument { argument: 9 })
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.state = StateDependencyContract::new(
        vec![PointStateDependency {
            id: 1,
            target: StorageTarget::ExecutingContract,
            key: ByteExpression::new(vec![ByteExpressionSegment::Argument(9)])
                .expect("expression is non-empty"),
            requirement: ReadRequirement::Present,
        }],
        Vec::<RangeStateDependency>::new(),
        Vec::<NativeCacheDependency>::new(),
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::UndeclaredExpressionInput)
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.gas_steps = Arc::from([GasStepContract {
        id: 0,
        amount: GasAmount::Fixed(1),
        exhaustion_fault: 9,
    }]);
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::UndeclaredGasFault { step: 0, fault: 9 })
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.faults = Arc::from([FaultContract {
        id: 0,
        class: FaultClass::InvalidOperation,
        effects: FaultEffectDisposition::Discard,
    }]);
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::InvalidGasFaultClass { step: 0 })
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.effects = EffectContract::new(
        StackEffectContract::new(1, 2, vec![output_buffer()]),
        Vec::<HostEffectContract>::new(),
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::StackConsumptionMismatch {
            consumed: 1,
            declared: 2,
        })
    );
}

#[test]
fn candidate_validation_rejects_ambiguous_context_and_notification_contracts() {
    let limits = CandidateContractLimits::DEFAULT;
    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.eligibility = InvocationEligibility::new(
        parts.eligibility.arguments().to_vec(),
        parts.eligibility.slots().to_vec(),
        vec![
            ContextDependency::CallFlags {
                required: 1,
                forbidden: 0,
            },
            ContextDependency::CallFlags {
                required: 2,
                forbidden: 0,
            },
        ],
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::DuplicateContextDependency)
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.eligibility = InvocationEligibility::new(
        parts.eligibility.arguments().to_vec(),
        parts.eligibility.slots().to_vec(),
        vec![ContextDependency::CallFlags {
            required: 1,
            forbidden: 1,
        }],
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::InvalidContextConstraint)
    );

    let mut parts = base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly);
    parts.effects = EffectContract::new(
        StackEffectContract::new(2, 3, vec![output_buffer()]),
        vec![HostEffectContract::Notification {
            emitter: ContractTarget::ExecutingContract,
            event_name: Arc::from([0xFF]),
            max_state_items: 1,
        }],
    );
    assert_eq!(
        CandidateContract::try_new(parts, limits),
        Err(CandidateContractError::InvalidNotificationEffect)
    );
}

#[test]
fn candidate_and_registry_bounds_are_hard_failures() {
    let mut limits = CandidateContractLimits::DEFAULT;
    limits.max_arguments = 1;
    assert_eq!(
        CandidateContract::try_new(
            base_parts(1, 1, base_execution(), CandidateAuthority::ShadowOnly),
            limits,
        ),
        Err(CandidateContractError::LimitExceeded {
            section: "arguments",
            actual: 2,
            maximum: 1,
        })
    );

    let first = base_candidate();
    let second = CandidateContract::try_new(
        base_parts(
            2,
            1,
            execution_key(
                &[0x10, 0x11, 0x13, 0x40],
                1,
                ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1),
                hardforks(),
                TriggerType::APPLICATION,
                Some(deployed_contract(0xCB)),
            ),
            CandidateAuthority::ShadowOnly,
        ),
        CandidateContractLimits::DEFAULT,
    )
    .expect("second candidate is valid");
    let entry_limits = SpecializationRegistryLimits {
        max_candidates: 1,
        ..SpecializationRegistryLimits::DEFAULT
    };
    assert_eq!(
        SpecializationRegistry::try_new([first.clone(), second], entry_limits)
            .expect_err("entry bound must reject"),
        RegistryBuildError::CandidateCapacity { maximum: 1 }
    );

    let byte_limits = SpecializationRegistryLimits {
        max_contract_bytes: first.accounted_bytes() - 1,
        ..SpecializationRegistryLimits::DEFAULT
    };
    assert_eq!(
        SpecializationRegistry::try_new([first.clone()], byte_limits)
            .expect_err("byte bound must reject"),
        RegistryBuildError::ByteCapacity {
            required: first.accounted_bytes(),
            maximum: first.accounted_bytes() - 1,
        }
    );

    let registry =
        SpecializationRegistry::try_new([first.clone()], SpecializationRegistryLimits::DEFAULT)
            .expect("registry is valid");
    assert_eq!(
        registry.snapshot(),
        RegistrySnapshot {
            candidates: 1,
            contract_bytes: first.accounted_bytes(),
            max_candidates: SpecializationRegistryLimits::DEFAULT.max_candidates,
            max_contract_bytes: SpecializationRegistryLimits::DEFAULT.max_contract_bytes,
        }
    );
}

#[test]
fn registry_rejects_duplicate_execution_and_candidate_versions() {
    let first = base_candidate();
    let duplicate_execution = CandidateContract::try_new(
        base_parts(
            2,
            1,
            first.identity().execution().clone(),
            CandidateAuthority::ShadowOnly,
        ),
        CandidateContractLimits::DEFAULT,
    )
    .expect("candidate itself is valid");
    assert_eq!(
        SpecializationRegistry::try_new(
            [first.clone(), duplicate_execution],
            SpecializationRegistryLimits::DEFAULT,
        )
        .expect_err("duplicate execution must reject"),
        RegistryBuildError::DuplicateExecutionIdentity
    );

    let duplicate_version = CandidateContract::try_new(
        base_parts(
            1,
            1,
            execution_key(
                &[0x10, 0x11, 0x13, 0x40],
                1,
                ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1),
                hardforks(),
                TriggerType::APPLICATION,
                Some(deployed_contract(0xCB)),
            ),
            CandidateAuthority::ShadowOnly,
        ),
        CandidateContractLimits::DEFAULT,
    )
    .expect("candidate itself is valid");
    assert_eq!(
        SpecializationRegistry::try_new(
            [first, duplicate_version],
            SpecializationRegistryLimits::DEFAULT,
        )
        .expect_err("duplicate candidate version must reject"),
        RegistryBuildError::DuplicateCandidateVersion {
            candidate_id: CandidateId::new(1),
            candidate_version: CandidateVersion::new(1),
        }
    );
}
