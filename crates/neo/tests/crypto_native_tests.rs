use neo_core::smart_contract::native::crypto_lib::CryptoLib;

fn decode_hex(input: &str) -> Vec<u8> {
    hex::decode(input).expect("valid hex")
}

#[test]
fn recover_secp256k1_signature_with_v() {
    let hash = decode_hex("0620d74975ea9f97c605e00aeb03c3fbd77fdd0536f9725cd37bb457bcea1b78");
    let signature = decode_hex("60eae22780db97d1a7a09a0da108920c0f0b8d4ba743493ca56c7a55141ac4013efe05933b811a5b8eba4c895df0768f5064e6ea41432d9df7a26389b815a60801");
    let contract = CryptoLib::new();
    let result = contract
        .recover_secp256k1(&[hash.clone(), signature])
        .unwrap();
    assert_eq!(
        result,
        decode_hex("03a1f60fe57bdbaed698a15a9aaf2d779117152ad0966b45700b89a73c51b9ad63")
    );
}

#[test]
fn recover_secp256k1_compact_signature() {
    let hash = decode_hex("0620d74975ea9f97c605e00aeb03c3fbd77fdd0536f9725cd37bb457bcea1b78");
    let compact = decode_hex("60eae22780db97d1a7a09a0da108920c0f0b8d4ba743493ca56c7a55141ac4013efe05933b811a5b8eba4c895df0768f5064e6ea41432d9df7a26389b815a608");
    let contract = CryptoLib::new();
    let result = contract
        .recover_secp256k1(&[hash.clone(), compact])
        .unwrap();
    assert_eq!(
        result,
        decode_hex("03a1f60fe57bdbaed698a15a9aaf2d779117152ad0966b45700b89a73c51b9ad63")
    );
}
