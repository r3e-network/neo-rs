use super::*;
use neo_crypto::Secp256r1Crypto;

fn test_pubkey(byte: u8) -> ECPoint {
    let mut private_key = [0u8; 32];
    private_key[31] = byte.max(1);
    let public = Secp256r1Crypto::derive_public_key(&private_key).expect("derive key");
    ECPoint::from_bytes(&public).expect("decode point")
}

/// A single-validator (1-of-1) commit assembles into a block whose header
/// carries the correct Merkle root, `NextConsensus` (= the 1-of-1 multi-sig
/// script hash), and a witness pairing that multi-sig with the pushed
/// commit signature.
#[test]
fn assemble_block_builds_header_and_multisig_witness() {
    let pubkey = test_pubkey(1);
    let signature = vec![0xABu8; 64];
    let data = BlockData {
        block_index: 1,
        timestamp: 12_345,
        nonce: 7,
        primary_index: 0,
        transaction_hashes: Vec::new(),
        signatures: vec![(0u8, signature.clone())],
        validator_pubkeys: vec![pubkey.clone()],
        required_signatures: 1,
        next_consensus: UInt160::from_script(&ConsensusBlockFields::multisig_verification_script(
            std::slice::from_ref(&pubkey),
        )),
    };

    let block = data
        .assemble_block(0, UInt256::zero(), Vec::new())
        .expect("assemble");

    // Header fields carried through from the committed consensus data.
    assert_eq!(block.header.index(), 1);
    assert_eq!(block.header.primary_index(), 0);
    assert_eq!(block.header.timestamp(), 12_345);

    // Merkle root of an empty block is the zero hash.
    assert_eq!(*block.header.merkle_root(), UInt256::zero());

    // NextConsensus == hash of the 1-of-1 multi-sig over the validator key,
    // and that script is the witness verification script.
    let expected_script = ConsensusBlockFields::multisig_verification_script(&[pubkey]);
    assert_eq!(
        *block.header.next_consensus(),
        UInt160::from_script(&expected_script)
    );
    assert_eq!(block.header.witness.verification_script, expected_script);

    // The invocation script pushes the single commit signature.
    assert!(
        block
            .header
            .witness
            .invocation_script
            .windows(signature.len())
            .any(|w| w == signature),
        "invocation script must contain the pushed commit signature"
    );
}

/// Assembly fails cleanly when fewer commit signatures than required are
/// present (rather than producing an unverifiable witness).
#[test]
fn assemble_block_rejects_insufficient_signatures() {
    let data = BlockData {
        block_index: 2,
        timestamp: 1,
        nonce: 0,
        primary_index: 0,
        transaction_hashes: Vec::new(),
        signatures: Vec::new(),
        validator_pubkeys: vec![test_pubkey(1), test_pubkey(2), test_pubkey(3)],
        required_signatures: 2,
        next_consensus: UInt160::zero(),
    };
    assert!(data.assemble_block(0, UInt256::zero(), Vec::new()).is_err());
}

#[test]
fn assemble_block_uses_committed_next_consensus_address() {
    let pubkey = test_pubkey(1);
    let committed_next_consensus = UInt160::from_bytes(&[0x42; 20]).expect("test next consensus");
    let data = BlockData {
        block_index: 3,
        timestamp: 12_345,
        nonce: 7,
        primary_index: 0,
        transaction_hashes: Vec::new(),
        signatures: vec![(0u8, vec![0xABu8; 64])],
        validator_pubkeys: vec![pubkey.clone()],
        required_signatures: 1,
        next_consensus: committed_next_consensus,
    };

    let block = data
        .assemble_block(0, UInt256::zero(), Vec::new())
        .expect("assemble");

    assert_eq!(*block.header.next_consensus(), committed_next_consensus);
    assert_eq!(
        block.header.witness.verification_script,
        ConsensusBlockFields::multisig_verification_script(&[pubkey]),
        "the consensus address can differ from the current-round witness script"
    );
}
