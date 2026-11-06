//! Block validation protocol compliance tests using real MainNet wire payloads.
//!
//! `block_vectors.json` is captured from a Neo v3.10.2 RPC node. The vectors
//! cover genesis, early transaction execution, a historical state-root repro,
//! and the Faun/Gorgon activation blocks.

#[cfg(test)]
mod tests {
    use ::neo_ledger::validation::rules::{
        validate_block_size_raw, validate_merkle_root, validate_transaction_count_raw,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use neo_core::UInt256;
    use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
    use neo_core::network::p2p::payloads::Block;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        useragent: String,
        blocks: Vec<BlockVector>,
    }

    #[derive(Deserialize)]
    struct BlockVector {
        height: u32,
        hash: String,
        #[serde(rename = "hex")]
        base64: String,
    }

    fn fixture() -> Fixture {
        let fixture: Fixture = serde_json::from_str(include_str!("../../block_vectors.json"))
            .expect("valid C# block fixture");
        assert_eq!(fixture.useragent, "/Neo:3.10.2/");
        assert!(
            !fixture.blocks.is_empty(),
            "block fixture must not be empty"
        );
        fixture
    }

    fn decode_and_validate(vector: &BlockVector) -> Block {
        let wire = BASE64
            .decode(&vector.base64)
            .expect("valid Base64 block payload");
        let mut reader = MemoryReader::new(&wire);
        let mut block =
            <Block as Serializable>::deserialize(&mut reader).expect("C# block must deserialize");

        assert_eq!(block.index(), vector.height);
        assert_eq!(block.hash(), UInt256::parse(&vector.hash).expect("hash"));
        assert_eq!(Serializable::size(&block), wire.len());
        validate_block_size_raw(wire.len()).expect("valid wire size");
        validate_transaction_count_raw(block.transactions.len()).expect("valid tx count");
        validate_merkle_root(block.merkle_root(), &block.transactions)
            .expect("valid transaction Merkle root");

        let mut writer = BinaryWriter::new();
        block.serialize(&mut writer).expect("serialize block");
        assert_eq!(writer.into_bytes(), wire, "wire round-trip must be exact");
        block
    }

    #[test]
    fn test_block_validation_vectors() {
        for vector in &fixture().blocks {
            decode_and_validate(vector);
        }
    }

    #[test]
    fn test_genesis_block_validation() {
        let fixture = fixture();
        let genesis = fixture
            .blocks
            .iter()
            .find(|vector| vector.height == 0)
            .expect("genesis fixture");
        let block = decode_and_validate(genesis);
        assert_eq!(block.header.version(), 0);
        assert_eq!(block.header.prev_hash(), &UInt256::zero());
        assert!(block.transactions.is_empty());
        assert_eq!(block.merkle_root(), &UInt256::zero());
    }
}
