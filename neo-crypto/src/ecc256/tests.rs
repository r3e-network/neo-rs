use super::{Keypair, PrivateKey, PublicKey, KEY_SIZE};
use alloc::vec::Vec;
use hex_literal::hex;
use neo_base::encoding::{NeoDecode, NeoEncode, SliceReader};
use rand::{rngs::StdRng, SeedableRng};

#[test]
fn compressed_roundtrip() {
    let pk = PublicKey::from_sec1_bytes(&hex!(
        "026e4bd1ab2b358fa6afa7e7f61f1c5d6b1fbcf91f55c2e1e7dda3297e4a8bba03"
    ))
    .unwrap();

    let mut buf = Vec::new();
    pk.neo_encode(&mut buf);
    let mut reader = SliceReader::new(buf.as_slice());
    let decoded = PublicKey::neo_decode(&mut reader).unwrap();
    assert_eq!(decoded, pk);
}

#[test]
fn keypair_from_private_matches_public() {
    let private = PrivateKey::from_slice(&[0x11; KEY_SIZE]).unwrap();
    let keypair = Keypair::from_private(private.clone()).unwrap();
    let derived = PublicKey::from_sec1_bytes(&keypair.public_key.to_compressed()).unwrap();
    assert_eq!(derived, keypair.public_key);
    assert_eq!(keypair.private_key, private);
}

#[test]
fn keypair_generate_produces_valid_keys() {
    let mut rng = StdRng::seed_from_u64(7);
    let keypair = Keypair::generate(&mut rng);
    let public = &keypair.public_key;
    let compressed = public.to_compressed();
    let decoded = PublicKey::from_sec1_bytes(&compressed).unwrap();
    assert_eq!(decoded, *public);
    assert_eq!(keypair.private_key.as_be_bytes().len(), KEY_SIZE);
}
