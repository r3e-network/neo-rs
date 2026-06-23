use super::*;

// 33-byte compressed secp256r1 public keys (valid points) used as fixtures.
fn key_a() -> Vec<u8> {
    hex_to_bytes("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
}
fn key_b() -> Vec<u8> {
    hex_to_bytes("02103a7f7dd016558597f7960d27c516a4394fd968b9e65155eb4b013e4040406e")
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn bft_threshold_matches_csharp_formula() {
    // C# `M = N - (N - 1) / 3`.
    assert_eq!(RedeemScript::bft_threshold(0), 0);
    assert_eq!(RedeemScript::bft_threshold(1), 1);
    assert_eq!(RedeemScript::bft_threshold(4), 3);
    assert_eq!(RedeemScript::bft_threshold(7), 5);
    assert_eq!(RedeemScript::bft_threshold(21), 15);
}

#[test]
fn bft_address_is_the_multisig_script_hash_or_none() {
    let keys: Vec<ECPoint> = [key_a(), key_b()]
        .iter()
        .map(|k| ECPoint::from_bytes(k).expect("valid key"))
        .collect();
    let addr = RedeemScript::bft_address(&keys).expect("non-empty -> Some");
    let m = RedeemScript::bft_threshold(keys.len());
    let script = RedeemScript::multi_sig_redeem_script_from_points(m, &keys).unwrap();
    assert_eq!(addr, neo_primitives::UInt160::from_script(&script));
    assert_eq!(RedeemScript::bft_address(&[]), None);
}

#[test]
fn signature_script_has_csharp_layout() {
    let pk = key_a();
    let script = RedeemScript::signature_redeem_script(&pk);
    assert_eq!(script.len(), 40);
    assert_eq!(script[0], OpCode::PUSHDATA1.byte());
    assert_eq!(script[1], 33);
    assert_eq!(&script[2..35], &pk[..]);
    assert_eq!(script[35], OpCode::SYSCALL.byte());
    assert_eq!(&script[36..40], &syscall_hash("System.Crypto.CheckSig"));
    assert!(RedeemScript::is_signature_contract(&script));
    assert!(!RedeemScript::is_multi_sig_contract(&script));
}

#[test]
fn multisig_script_from_keys_matches_points() {
    let keys = vec![key_a(), key_b()];
    let from_keys = RedeemScript::multi_sig_redeem_script_from_keys(2, &keys).expect("from keys");

    let points: Vec<ECPoint> = keys
        .iter()
        .map(|k| ECPoint::from_bytes(k).unwrap())
        .collect();
    let from_points =
        RedeemScript::multi_sig_redeem_script_from_points(2, &points).expect("from points");

    assert_eq!(from_keys, from_points);
    assert!(RedeemScript::is_multi_sig_contract(&from_keys));
    assert!(!RedeemScript::is_signature_contract(&from_keys));
    // C# 2-of-2 layout: PUSH2 .. PUSH2 SYSCALL CheckMultisig
    assert_eq!(from_keys[0], OpCode::PUSH2.byte());
    assert_eq!(from_keys[from_keys.len() - 5], OpCode::SYSCALL.byte());
    assert_eq!(
        &from_keys[from_keys.len() - 4..],
        &syscall_hash("System.Crypto.CheckMultisig")
    );
}

#[test]
fn multisig_key_order_is_canonical() {
    // Output must be independent of input order (keys are sorted ascending).
    let forward = RedeemScript::multi_sig_redeem_script_from_keys(2, &[key_a(), key_b()]).unwrap();
    let reverse = RedeemScript::multi_sig_redeem_script_from_keys(2, &[key_b(), key_a()]).unwrap();
    assert_eq!(forward, reverse);
}

#[test]
fn multisig_rejects_invalid_params() {
    assert!(RedeemScript::multi_sig_redeem_script_from_keys(0, &[key_a()]).is_err());
    assert!(RedeemScript::multi_sig_redeem_script_from_keys(3, &[key_a(), key_b()]).is_err());
    assert!(RedeemScript::multi_sig_redeem_script_from_points(1, &[]).is_err());
}

#[test]
fn multisig_recognizer_rejects_invalid_public_key_points() {
    let mut script = Vec::new();
    script.push(OpCode::PUSH1.byte());
    script.push(OpCode::PUSHDATA1.byte());
    script.push(33);
    script.extend_from_slice(&[0u8; 33]);
    script.push(OpCode::PUSH1.byte());
    script.push(OpCode::SYSCALL.byte());
    script.extend_from_slice(&RedeemScript::check_multisig_hash());

    assert!(
        RedeemScript::parse_multi_sig_contract(&script).is_none(),
        "C# Helper.IsMultiSigContract decodes every ECPoint and rejects invalid public keys"
    );
    assert!(!RedeemScript::is_multi_sig_contract(&script));
}

/// C# `Contract.CreateMultiSigRedeemScript` allows up to 1024 keys; the
/// raw-bytes builder must too (the `CreateMultisigAccount` interop and large
/// committee multisigs use it). Previously it capped at 16, faulting where C#
/// succeeds. A >16-key script is accepted and the from_keys path matches the
/// from_points path (which the genesis 21-key committee hash already pins).
#[test]
fn multisig_from_keys_allows_more_than_16_keys() {
    // 17 deterministic distinct keys (derive from small scalars).
    use neo_crypto::Secp256r1Crypto;
    let keys: Vec<Vec<u8>> = (1u8..=17)
        .map(|i| {
            let mut sk = [0u8; 32];
            sk[31] = i;
            Secp256r1Crypto::derive_public_key(&sk).unwrap().to_vec()
        })
        .collect();
    let script = RedeemScript::multi_sig_redeem_script_from_keys(11, &keys)
        .expect("17-key multisig must build (C# allows up to 1024)");
    let points: Vec<ECPoint> = keys
        .iter()
        .map(|k| ECPoint::from_bytes(k).unwrap())
        .collect();
    let from_points = RedeemScript::multi_sig_redeem_script_from_points(11, &points).unwrap();
    assert_eq!(script, from_points, "from_keys must equal from_points");
    // The recognizer must round-trip a >16-of-n script (m and n via PUSHINT8).
    let (m, parsed) = RedeemScript::parse_multi_sig_contract(&script).expect("recognize 11-of-17");
    assert_eq!(m, 11);
    assert_eq!(parsed.len(), 17);
}
