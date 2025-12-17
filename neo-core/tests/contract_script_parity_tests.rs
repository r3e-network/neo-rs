use neo_core::cryptography::ECPoint;
use neo_core::smart_contract::Contract;
use neo_core::wallets::key_pair::KeyPair;
use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;

fn fixed_keypair(hex_privkey_32: &str) -> KeyPair {
    let bytes = hex::decode(hex_privkey_32).expect("hex private key");
    assert_eq!(bytes.len(), 32);
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&bytes);
    KeyPair::from_private_key(&buf).expect("keypair from private key")
}

#[test]
fn signature_redeem_script_matches_opcode_layout() {
    // Deterministic key to keep expected bytes stable.
    let key = fixed_keypair("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20");
    let pubkey_bytes = key.compressed_public_key();
    assert_eq!(pubkey_bytes.len(), 33);

    let pubkey_point = ECPoint::decode_secp256r1(&pubkey_bytes).expect("pubkey point");
    let script = Contract::create_signature_redeem_script(pubkey_point);

    let syscall_hash = ScriptBuilder::hash_syscall("System.Crypto.CheckSig").expect("syscall hash");
    let mut expected = Vec::with_capacity(1 + 1 + 33 + 1 + 4);
    expected.push(OpCode::PUSHDATA1 as u8);
    expected.push(0x21);
    expected.extend_from_slice(&pubkey_bytes);
    expected.push(OpCode::SYSCALL as u8);
    expected.extend_from_slice(&syscall_hash.to_le_bytes());

    assert_eq!(script, expected);
}

#[test]
fn multisig_redeem_script_sorts_pubkeys_like_csharp() {
    // Two deterministic keys, intentionally provided out-of-order.
    let k1 = fixed_keypair("1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100");
    let k2 = fixed_keypair("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");

    let mut keys: Vec<ECPoint> = vec![
        ECPoint::decode_secp256r1(&k1.compressed_public_key()).expect("pubkey point"),
        ECPoint::decode_secp256r1(&k2.compressed_public_key()).expect("pubkey point"),
    ];
    keys.sort(); // uses C#-compatible ECPoint ordering (X then Y)

    let script = Contract::create_multi_sig_redeem_script(2, &keys);

    let syscall_hash =
        ScriptBuilder::hash_syscall("System.Crypto.CheckMultisig").expect("syscall hash");
    let mut expected = Vec::with_capacity(1 + (1 + 1 + 33) * 2 + 1 + 1 + 4);
    expected.push(OpCode::PUSH2 as u8);

    for pk in &keys {
        let pk = pk.encode_point(true).expect("compressed pubkey");
        expected.push(OpCode::PUSHDATA1 as u8);
        expected.push(0x21);
        expected.extend_from_slice(&pk);
    }

    expected.push(OpCode::PUSH2 as u8);
    expected.push(OpCode::SYSCALL as u8);
    expected.extend_from_slice(&syscall_hash.to_le_bytes());

    assert_eq!(script, expected);
}
