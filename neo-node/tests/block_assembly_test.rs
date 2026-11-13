//! Integration test for block assembly logic

use neo_crypto::ECPoint;
use neo_crypto::{ECCurve, ecc::generate_keypair};
use neo_primitives::UInt256;
use neo_protocol::payloads::Block;
use neo_vm::OpCode;

/// Consensus block data consumed by the assembly workflow.
///
/// The former `neo_consensus::BlockData` type no longer exists in the
/// workspace (there is no `neo-consensus` crate anymore); this local fixture
/// preserves the same shape so the assembly workflow below is unchanged.
struct BlockData {
    block_index: u32,
    timestamp: u64,
    nonce: u64,
    primary_index: u8,
    /// Kept for parity with the consensus fixture shape; not consumed here.
    #[allow(dead_code)]
    transaction_hashes: Vec<UInt256>,
    signatures: Vec<(u8, Vec<u8>)>,
    validator_pubkeys: Vec<ECPoint>,
    required_signatures: usize,
}

#[tokio::test]
async fn test_complete_block_assembly_workflow() {
    // Generate 4 validator key pairs
    let mut validator_pubkeys = Vec::new();

    for _ in 0..4 {
        let (_private_key, public_key) = generate_keypair(ECCurve::Secp256r1).unwrap();
        validator_pubkeys.push(public_key);
    }

    // Create mock signatures (3 out of 4 validators sign)
    // In real scenario, these would be actual ECDSA signatures
    let signatures = vec![
        (0u8, vec![0xAAu8; 64]), // Validator 0 signature
        (1u8, vec![0xBBu8; 64]), // Validator 1 signature
        (2u8, vec![0xCCu8; 64]), // Validator 2 signature
    ];

    // Create BlockData from consensus
    let block_data = BlockData {
        block_index: 12345,
        timestamp: 1700000000,
        nonce: 0xDEADBEEFCAFEBABE,
        primary_index: 1,
        transaction_hashes: vec![],
        signatures,
        validator_pubkeys,
        required_signatures: 3,
    };

    // Assemble block (simulating validator service logic)
    let block = assemble_test_block(block_data).await.unwrap();

    // Verify block structure
    assert_eq!(block.index(), 12345);
    assert_eq!(block.timestamp(), 1700000000);
    assert_eq!(block.nonce(), 0xDEADBEEFCAFEBABE);
    assert_eq!(block.primary_index(), 1);

    // Verify witness
    let witness = block.witness();
    assert!(!witness.invocation_script.is_empty());
    assert!(!witness.verification_script.is_empty());

    // Verify invocation script: 3 signatures * 66 bytes = 198 bytes
    assert_eq!(witness.invocation_script.len(), 198);

    // Verify verification script format
    assert_eq!(witness.verification_script[0], OpCode::PUSH3.byte()); // PUSH3 for M=3

    println!("✓ Block assembly test passed");
    println!("  Block index: {}", block.index());
    println!(
        "  Invocation script size: {} bytes",
        witness.invocation_script.len()
    );
    println!(
        "  Verification script size: {} bytes",
        witness.verification_script.len()
    );
}

// Helper function to simulate block assembly
async fn assemble_test_block(block_data: BlockData) -> anyhow::Result<Block> {
    use neo_protocol::payloads::{Block, Header};
    use neo_protocol::witness::Witness;
    use neo_smart_contract::helper::Helper;
    use neo_vm::OpCode;

    // Build invocation script
    let mut invocation = Vec::new();
    let mut sorted_sigs = block_data.signatures.clone();
    sorted_sigs.sort_by_key(|(idx, _)| *idx);

    for (_, signature) in &sorted_sigs {
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(signature);
    }

    // Build verification script
    let pubkey_bytes: Vec<Vec<u8>> = block_data
        .validator_pubkeys
        .iter()
        .map(|pk| pk.encoded().clone())
        .collect();

    let verification =
        Helper::try_multi_sig_redeem_script(block_data.required_signatures, &pubkey_bytes)?;

    // Create witness
    let witness = Witness::new_with_scripts(invocation, verification);

    // Create block
    let mut header = Header::new();
    header.set_version(0);
    header.set_index(block_data.block_index);
    header.set_timestamp(block_data.timestamp);
    header.set_nonce(block_data.nonce);
    header.set_primary_index(block_data.primary_index);

    let mut block = Block::new();
    block.header = header;
    block.header.witness = witness;

    Ok(block)
}
