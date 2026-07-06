use super::*;
use neo_config::ProtocolSettings;
use neo_primitives::{TriggerType, UInt256};
use neo_storage::persistence::DataCache;
use std::str::FromStr;
use std::sync::Arc;

fn test_engine() -> ApplicationEngine {
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        1_000_000,
        None,
        None,
    )
    .expect("engine builds")
}

fn valid_public_key() -> Vec<u8> {
    hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
        .expect("valid key hex")
}

fn invalid_public_key() -> Vec<u8> {
    let mut key = vec![0x04; 33];
    key[1] = 0x01;
    key
}

#[test]
fn contract_call_trace_filter_matches_exact_transaction_hash() {
    let target =
        UInt256::from_str("0x5e0cbae4dcfd0e97084b7a79fb29e2dbec2ba860d34717948f5e3809d9ccb4d3")
            .expect("target hash");
    let other =
        UInt256::from_str("0xdb40cbead625dc9211712875f3e69d17550c1d6b0452efd548809525577f14bd")
            .expect("other hash");

    assert!(contract_call_trace_filter_matches(
        Some(target),
        false,
        Some(target),
    ));
    assert!(!contract_call_trace_filter_matches(
        Some(target),
        false,
        Some(other),
    ));
    assert!(contract_call_trace_filter_matches(None, true, Some(other)));
    assert!(!contract_call_trace_filter_matches(
        None,
        false,
        Some(other)
    ));
}

#[test]
fn native_call_trace_filter_matches_exact_transaction_hash() {
    let target =
        UInt256::from_str("0x5e0cbae4dcfd0e97084b7a79fb29e2dbec2ba860d34717948f5e3809d9ccb4d3")
            .expect("target hash");
    let other =
        UInt256::from_str("0xdb40cbead625dc9211712875f3e69d17550c1d6b0452efd548809525577f14bd")
            .expect("other hash");

    assert!(native_call_trace_filter_matches(
        Some(target),
        false,
        Some(target),
    ));
    assert!(!native_call_trace_filter_matches(
        Some(target),
        false,
        Some(other),
    ));
    assert!(native_call_trace_filter_matches(None, true, Some(other)));
    assert!(!native_call_trace_filter_matches(None, false, Some(other)));
}

#[test]
fn create_standard_account_rejects_invalid_ecpoint_before_dynamic_fee() {
    let mut engine = test_engine();

    assert!(
        engine
            .create_standard_account(&invalid_public_key())
            .is_err()
    );
    assert_eq!(
        engine.fee_consumed(),
        0,
        "C# converts ECPoint before entering CreateStandardAccount"
    );
}

#[test]
fn create_multisig_account_rejects_invalid_ecpoint_before_dynamic_fee() {
    let mut engine = test_engine();
    let keys = vec![StackItem::from_byte_string(invalid_public_key())];

    assert!(engine.create_multisig_account(1, keys).is_err());
    assert_eq!(
        engine.fee_consumed(),
        0,
        "C# converts ECPoint[] before entering CreateMultisigAccount"
    );
}

#[test]
fn create_multisig_account_charges_fee_before_invalid_threshold_fault() {
    let mut engine = test_engine();
    let keys = vec![StackItem::from_byte_string(valid_public_key())];

    assert!(engine.create_multisig_account(-1, keys).is_err());
    assert!(
        engine.fee_consumed() > 0,
        "C# charges inside CreateMultisigAccount before m/n validation"
    );
}

#[test]
fn decode_native_result_array_empty_is_null() {
    let item = decode_native_result(ContractParameterType::Array, Vec::new())
        .expect("decode")
        .expect("stack item");
    assert!(item.is_null());
}

#[test]
fn decode_native_result_any_empty_is_null() {
    let item = decode_native_result(ContractParameterType::Any, Vec::new())
        .expect("decode")
        .expect("stack item");
    assert!(item.is_null());
}

#[test]
fn decode_native_result_any_invalid_payload_preserves_raw_bytes() {
    let item = decode_native_result(ContractParameterType::Any, vec![0xff])
        .expect("decode")
        .expect("stack item");
    assert_eq!(item.as_bytes().expect("bytes"), vec![0xff]);
}

#[test]
fn decode_native_result_any_deserializes_stack_item_payloads() {
    let original = StackItem::from_array(vec![StackItem::from_int(BigInt::from(1u8))]);
    let encoded =
        BinarySerializer::serialize(&original, &ExecutionEngineLimits::default()).expect("encode");
    let decoded = decode_native_result(ContractParameterType::Any, encoded)
        .expect("decode")
        .expect("stack item");
    assert!(matches!(decoded, StackItem::Array(_)));
}

#[test]
fn decode_native_result_array_payload_roundtrips() {
    let original = StackItem::from_array(vec![StackItem::from_int(BigInt::from(1u8))]);
    let encoded =
        BinarySerializer::serialize(&original, &ExecutionEngineLimits::default()).expect("encode");
    let decoded = decode_native_result(ContractParameterType::Array, encoded)
        .expect("decode")
        .expect("stack item");
    assert!(matches!(decoded, StackItem::Array(_)));
}

#[test]
fn decode_native_result_interop_wraps_bls_point_lengths() {
    // A 4-byte InteropInterface payload is an iterator handle.
    let iter = decode_native_result(ContractParameterType::InteropInterface, vec![1, 0, 0, 0])
        .expect("decode")
        .expect("stack item");
    assert!(iter.as_interface::<IteratorInterop>().is_ok());

    // 48 / 96 / 576-byte payloads are BLS12-381 points → Bls12381Interop.
    for len in [G1_COMPRESSED_SIZE, G2_COMPRESSED_SIZE, GT_SIZE] {
        let bytes = vec![0u8; len];
        let item = decode_native_result(ContractParameterType::InteropInterface, bytes.clone())
            .expect("decode")
            .expect("stack item");
        let point = item
            .as_interface::<Bls12381Interop>()
            .expect("BLS interop wrapper");
        assert_eq!(point.bytes(), bytes.as_slice());
        // It is NOT an iterator (the two interop kinds are distinct).
        assert!(item.as_interface::<IteratorInterop>().is_err());
    }
}

#[test]
fn interop_bytes_round_trips_typed_objects_and_rejects_plain_bytestring() {
    // A Bls12381Interop operand unwraps back to its canonical bytes.
    let bytes = vec![7u8; GT_SIZE];
    let item = StackItem::from_interface(Bls12381Interop::new(bytes.clone()));
    assert_eq!(stack_item_to_interop_bytes(item).expect("bls bytes"), bytes);

    // An IteratorInterop operand encodes its handle id as 4 LE bytes.
    let iter = StackItem::from_interface(IteratorInterop::new(5));
    assert_eq!(
        stack_item_to_interop_bytes(iter).expect("iter id"),
        5u32.to_le_bytes()
    );

    // A plain ByteString is NOT a live interop object: C# faults when binding
    // an InteropInterface parameter from a non-interface item, so we must err
    // rather than silently accept the raw bytes.
    let raw = StackItem::from_byte_string(vec![0u8; GT_SIZE]);
    assert!(stack_item_to_interop_bytes(raw).is_err());
}
