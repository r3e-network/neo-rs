use super::*;
use ciborium::value::Value;

fn synthetic_payload(pcr0: Vec<u8>, pcr1: Vec<u8>, pcr2: Vec<u8>, with_optionals: bool) -> Vec<u8> {
    let pcr_map = Value::Map(vec![
        (Value::Integer(0.into()), Value::Bytes(pcr0)),
        (Value::Integer(1.into()), Value::Bytes(pcr1)),
        (Value::Integer(2.into()), Value::Bytes(pcr2)),
    ]);

    let mut entries = vec![
        (
            Value::Text("module_id".into()),
            Value::Text("i-0abc-enc01".into()),
        ),
        (Value::Text("digest".into()), Value::Text("SHA384".into())),
        (
            Value::Text("timestamp".into()),
            Value::Integer(1_700_000_000_000u64.into()),
        ),
        (Value::Text("pcrs".into()), pcr_map),
        (
            Value::Text("certificate".into()),
            Value::Bytes(vec![0x30, 0x82, 0x01, 0x02]),
        ),
        (
            Value::Text("cabundle".into()),
            Value::Array(vec![
                Value::Bytes(vec![0x30, 0x82, 0x02, 0x01]),
                Value::Bytes(vec![0x30, 0x82, 0x03, 0x01]),
            ]),
        ),
    ];

    if with_optionals {
        entries.push((
            Value::Text("public_key".into()),
            Value::Bytes(vec![0xAA; 8]),
        ));
        entries.push((
            Value::Text("user_data".into()),
            Value::Bytes(vec![0xBB; 16]),
        ));
        entries.push((Value::Text("nonce".into()), Value::Bytes(vec![0xCC; 12])));
    } else {
        entries.push((Value::Text("public_key".into()), Value::Null));
        entries.push((Value::Text("user_data".into()), Value::Null));
        entries.push((Value::Text("nonce".into()), Value::Null));
    }

    let map = Value::Map(entries);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&map, &mut buf).unwrap();
    buf
}

fn synthetic_cose(payload: Vec<u8>) -> Vec<u8> {
    let cose = Value::Array(vec![
        Value::Bytes(vec![0xA1, 0x01, 0x38, 0x22]),
        Value::Map(vec![]),
        Value::Bytes(payload),
        Value::Bytes(vec![0u8; 96]),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&cose, &mut buf).unwrap();
    buf
}

#[test]
fn parses_cose_sign1_envelope() {
    let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
    let cose_bytes = synthetic_cose(payload.clone());

    let envelope = parse_cose_sign1(&cose_bytes).unwrap();
    assert_eq!(envelope.payload, payload);
    assert_eq!(envelope.signature.len(), 96);
    assert!(!envelope.protected.is_empty());
}

#[test]
fn rejects_non_array_cose() {
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&Value::Text("nope".into()), &mut buf).unwrap();
    assert!(parse_cose_sign1(&buf).is_err());
}

#[test]
fn rejects_wrong_arity_cose() {
    let three = Value::Array(vec![
        Value::Bytes(vec![]),
        Value::Map(vec![]),
        Value::Bytes(vec![]),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&three, &mut buf).unwrap();
    assert!(parse_cose_sign1(&buf).is_err());
}

#[test]
fn parses_payload_with_optionals() {
    let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], true);
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();

    assert_eq!(doc.module_id, "i-0abc-enc01");
    assert_eq!(doc.digest, "SHA384");
    assert_eq!(doc.timestamp_ms, 1_700_000_000_000);
    assert_eq!(doc.pcrs.len(), 3);
    assert_eq!(doc.pcr(0).unwrap(), &[1u8; 48]);
    assert_eq!(doc.cabundle.len(), 2);
    assert_eq!(doc.public_key.as_deref(), Some([0xAA; 8].as_slice()));
    assert_eq!(doc.user_data.as_deref(), Some([0xBB; 16].as_slice()));
    assert_eq!(doc.nonce.as_deref(), Some([0xCC; 12].as_slice()));
}

#[test]
fn parses_payload_with_null_optionals() {
    let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
    assert!(doc.public_key.is_none());
    assert!(doc.user_data.is_none());
    assert!(doc.nonce.is_none());
}

#[test]
fn end_to_end_parse_from_cose() {
    let payload = synthetic_payload(vec![7u8; 48], vec![8u8; 48], vec![9u8; 48], true);
    let cose_bytes = synthetic_cose(payload);
    let doc = NitroAttestationDoc::parse(&cose_bytes).unwrap();
    assert_eq!(doc.module_id, "i-0abc-enc01");
    assert_eq!(doc.pcr(2).unwrap(), &[9u8; 48]);
}

#[test]
fn structural_validate_accepts_well_formed_doc() {
    let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
    doc.structural_validate(&NitroValidationOptions::default())
        .unwrap();
}

#[test]
fn structural_validate_rejects_zero_pcrs() {
    let payload = synthetic_payload(vec![0u8; 48], vec![0u8; 48], vec![0u8; 48], false);
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
    let err = doc
        .structural_validate(&NitroValidationOptions::default())
        .unwrap_err();
    assert!(matches!(err, TeeError::InvalidAttestationReport(_)));
}

#[test]
fn structural_validate_rejects_wrong_pcr_length() {
    let payload = synthetic_payload(vec![1u8; 47], vec![2u8; 48], vec![3u8; 48], false);
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
    assert!(
        doc.structural_validate(&NitroValidationOptions::default())
            .is_err()
    );
}

#[test]
fn structural_validate_enforces_pcr_pin() {
    let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();

    let mut opts = NitroValidationOptions {
        expected_pcr0: Some([1u8; 48]),
        ..NitroValidationOptions::default()
    };
    doc.structural_validate(&opts).unwrap();

    opts.expected_pcr0 = Some([9u8; 48]);
    assert!(doc.structural_validate(&opts).is_err());
}

#[test]
fn structural_validate_rejects_wrong_digest() {
    let map = Value::Map(vec![
        (Value::Text("module_id".into()), Value::Text("m".into())),
        (Value::Text("digest".into()), Value::Text("SHA256".into())),
        (Value::Text("timestamp".into()), Value::Integer(1u64.into())),
        (
            Value::Text("pcrs".into()),
            Value::Map(vec![(
                Value::Integer(0.into()),
                Value::Bytes(vec![1u8; 48]),
            )]),
        ),
        (Value::Text("certificate".into()), Value::Bytes(vec![0x30])),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&map, &mut buf).unwrap();
    let doc = NitroAttestationDoc::parse_payload(&buf).unwrap();
    assert!(
        doc.structural_validate(&NitroValidationOptions::default())
            .is_err()
    );
}

#[test]
fn parse_payload_rejects_missing_required_field() {
    let map = Value::Map(vec![
        (Value::Text("module_id".into()), Value::Text("m".into())),
        (Value::Text("digest".into()), Value::Text("SHA384".into())),
        (Value::Text("timestamp".into()), Value::Integer(1u64.into())),
        (
            Value::Text("pcrs".into()),
            Value::Map(vec![(
                Value::Integer(0.into()),
                Value::Bytes(vec![1u8; 48]),
            )]),
        ),
    ]);
    let mut buf = Vec::new();
    ciborium::ser::into_writer(&map, &mut buf).unwrap();
    assert!(NitroAttestationDoc::parse_payload(&buf).is_err());
}

#[test]
fn verify_pki_chain_is_stub() {
    let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
    let cose_bytes = synthetic_cose(payload.clone());
    let envelope = parse_cose_sign1(&cose_bytes).unwrap();
    let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
    assert_eq!(
        verify_pki_chain(&doc, &envelope, &NITRO_ROOT_G1_SHA256_FINGERPRINT),
        PkiVerification::Stub
    );
}
