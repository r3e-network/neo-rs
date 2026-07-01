use neo_execution::Helper;
use neo_primitives::UInt160;

/// Verifies our consensus-critical contract-hash derivation reproduces every
/// canonical Neo N3 native-contract hash.
///
/// In C# Neo v3.10.0 each native contract sets
/// `Hash = Helper.GetContractHash(UInt160.Zero, 0, Name)`, where
/// `Name => GetType().Name` (the class name). `GetContractHash` emits
/// `ABORT, push(sender), push(nefCheckSum), push(name)` and takes the script
/// hash (`sha256` then `ripemd160`). If our `ScriptBuilder` opcode/push
/// encoding or script-hash derivation diverged from C#, the derived values
/// would not match the well-known MainNet hashes pinned above — so this is a
/// genuine cross-implementation parity check, not a self-referential one.
#[test]
fn native_contract_hashes_match_csharp_derivation() {
    let zero = UInt160::default();
    for spec in crate::standard_native_contract_specs() {
        let derived = Helper::get_contract_hash(&zero, 0, spec.name);
        assert_eq!(
            derived, spec.hash,
            "get_contract_hash(0, 0, {:?}) diverged from the canonical Neo N3 hash",
            spec.name
        );
    }
}

/// All native-contract hashes must be distinct (a collision would let two
/// native contracts share storage and break consensus).
#[test]
fn native_contract_hashes_are_distinct() {
    let all = crate::standard_native_contract_specs();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            assert_ne!(
                all[i].hash, all[j].hash,
                "duplicate native contract hash for {} / {}",
                all[i].name, all[j].name
            );
        }
    }
}

/// Native-contract ids must be the contiguous negative sequence assigned by
/// C# in canonical catalog order. The id is part of native storage-key
/// derivation, so a wrong id silently diverges consensus.
#[test]
fn native_contract_ids_are_contiguous_in_csharp_order() {
    let specs = crate::standard_native_contract_specs();
    assert_eq!(specs.len(), crate::STANDARD_NATIVE_CONTRACT_COUNT);
    for (index, spec) in specs.iter().enumerate() {
        let expected_id = -((index as i32) + 1);
        assert_eq!(
            spec.id, expected_id,
            "{} id must match its canonical C# order",
            spec.name
        );
    }
}
