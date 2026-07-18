use super::*;
use neo_primitives::constants::MAINNET_MAGIC;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn base_protocol() -> ProtocolIdentity {
    ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::NEO_N3_V3_10_1)
}

fn base_hardforks() -> HardforkTableIdentity {
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

fn base_contract() -> ContractResolutionIdentity {
    ContractResolutionIdentity::new(UInt160::from([0x42; 20]), 17, 3, 0x1020_3040)
}

fn key(
    bytes: &[u8],
    entry_ip: u32,
    protocol: ProtocolIdentity,
    hardforks: HardforkTableIdentity,
    trigger: TriggerType,
    contract: Option<ContractResolutionIdentity>,
) -> ExecutionPlanKey {
    ExecutionPlanKey::new(
        Arc::<[u8]>::from(bytes),
        entry_ip,
        protocol,
        hardforks,
        trigger,
        contract,
    )
}

fn base_key() -> ExecutionPlanKey {
    key(
        &[0x10, 0x11, 0x40],
        1,
        base_protocol(),
        base_hardforks(),
        TriggerType::APPLICATION,
        Some(base_contract()),
    )
}

fn hash_of(value: &ExecutionPlanKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn equal_keys_use_exact_bytes_not_arc_or_script_object_identity() {
    let first = base_key();
    let second = base_key();

    assert_eq!(first, second);
    assert_eq!(hash_of(&first), hash_of(&second));
    assert_eq!(first.version(), ExecutionPlanKeyVersion::V1);
    assert_eq!(first.version().value(), 1);
    assert_eq!(first.script_len(), 3);
    assert_eq!(first.script_bytes(), &[0x10, 0x11, 0x40]);
}

#[test]
fn every_non_script_dependency_produces_a_distinct_key() {
    let baseline = base_key();

    let distinct = [
        key(
            baseline.script_bytes(),
            2,
            base_protocol(),
            base_hardforks(),
            TriggerType::APPLICATION,
            Some(base_contract()),
        ),
        key(
            baseline.script_bytes(),
            1,
            ProtocolIdentity::new(0x1234_5678, ProtocolVersion::NEO_N3_V3_10_1),
            base_hardforks(),
            TriggerType::APPLICATION,
            Some(base_contract()),
        ),
        key(
            baseline.script_bytes(),
            1,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::new(4, 10, 1)),
            base_hardforks(),
            TriggerType::APPLICATION,
            Some(base_contract()),
        ),
        key(
            baseline.script_bytes(),
            1,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::new(3, 11, 1)),
            base_hardforks(),
            TriggerType::APPLICATION,
            Some(base_contract()),
        ),
        key(
            baseline.script_bytes(),
            1,
            ProtocolIdentity::new(MAINNET_MAGIC, ProtocolVersion::new(3, 10, 2)),
            base_hardforks(),
            TriggerType::APPLICATION,
            Some(base_contract()),
        ),
        key(
            baseline.script_bytes(),
            1,
            base_protocol(),
            base_hardforks(),
            TriggerType::VERIFICATION,
            Some(base_contract()),
        ),
        key(
            baseline.script_bytes(),
            1,
            base_protocol(),
            base_hardforks(),
            TriggerType::APPLICATION,
            None,
        ),
    ];

    for candidate in distinct {
        assert_ne!(baseline, candidate);
    }
}

#[test]
fn every_known_hardfork_slot_and_applicability_are_keyed() {
    let baseline = key(
        &[0x40],
        0,
        base_protocol(),
        HardforkTableIdentity::unconfigured(),
        TriggerType::APPLICATION,
        None,
    );

    for (index, hardfork) in Hardfork::ALL.into_iter().enumerate() {
        let pending = HardforkTableIdentity::unconfigured().with_state(
            hardfork,
            HardforkPlanState::Pending {
                activation_height: 100 + index as u32,
            },
        );
        let active = HardforkTableIdentity::unconfigured().with_state(
            hardfork,
            HardforkPlanState::Active {
                activation_height: 100 + index as u32,
            },
        );
        let pending_key = key(
            &[0x40],
            0,
            base_protocol(),
            pending,
            TriggerType::APPLICATION,
            None,
        );
        let active_key = key(
            &[0x40],
            0,
            base_protocol(),
            active,
            TriggerType::APPLICATION,
            None,
        );

        assert_ne!(baseline, pending_key, "missing {hardfork:?} schedule");
        assert_ne!(pending_key, active_key, "missing {hardfork:?} state");
        assert_eq!(
            pending.state(hardfork).activation_height(),
            Some(100 + index as u32)
        );
        assert!(!pending.state(hardfork).is_active());
        assert!(active.state(hardfork).is_active());
    }

    let changed_height = HardforkTableIdentity::unconfigured().with_state(
        Hardfork::HfBasilisk,
        HardforkPlanState::Pending {
            activation_height: 101,
        },
    );
    let other_height = changed_height.with_state(
        Hardfork::HfBasilisk,
        HardforkPlanState::Pending {
            activation_height: 102,
        },
    );
    assert_ne!(changed_height, other_height);
}

#[test]
fn every_contract_resolution_component_is_keyed() {
    let baseline = base_key();
    let changed = [
        ContractResolutionIdentity::new(UInt160::from([0x43; 20]), 17, 3, 0x1020_3040),
        ContractResolutionIdentity::new(UInt160::from([0x42; 20]), 18, 3, 0x1020_3040),
        ContractResolutionIdentity::new(UInt160::from([0x42; 20]), 17, 4, 0x1020_3040),
        ContractResolutionIdentity::new(UInt160::from([0x42; 20]), 17, 3, 0x1020_3041),
    ];

    for contract in changed {
        let candidate = key(
            baseline.script_bytes(),
            baseline.entry_ip(),
            baseline.protocol(),
            baseline.hardforks(),
            baseline.trigger(),
            Some(contract),
        );
        assert_ne!(baseline, candidate);
    }

    let contract = baseline.contract().expect("base key has a contract");
    assert_eq!(contract.contract_hash(), UInt160::from([0x42; 20]));
    assert_eq!(contract.contract_id(), 17);
    assert_eq!(contract.update_counter(), 3);
    assert_eq!(contract.nef_checksum(), 0x1020_3040);
}

#[test]
fn exact_byte_verification_rejects_hash_collisions_and_mismatches() {
    let first = base_key();
    let mut colliding = key(
        &[0x10, 0x12, 0x40],
        first.entry_ip(),
        first.protocol(),
        first.hardforks(),
        first.trigger(),
        first.contract(),
    );
    colliding.script_hash = first.script_hash;

    assert_eq!(first.script_hash, colliding.script_hash);
    assert_eq!(first.script_len(), colliding.script_len());
    assert_eq!(hash_of(&first), hash_of(&colliding));
    assert_ne!(first, colliding);
    assert!(!first.matches_script(colliding.script_hash(), colliding.script_bytes()));
    assert!(!first.matches_script(&[0xFF; 20], first.script_bytes()));

    let mut set = HashSet::new();
    assert!(set.insert(first));
    assert!(set.insert(colliding));
    assert_eq!(set.len(), 2);
}

#[test]
fn key_schema_version_participates_in_identity() {
    let first = base_key();
    let mut future = first.clone();
    future.version = ExecutionPlanKeyVersion(2);

    assert_ne!(first, future);
    assert_ne!(hash_of(&first), hash_of(&future));
}
