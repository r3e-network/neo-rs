//! Protocol compliance tests for Neo N3 v3.10.1
//!
//! The block vectors exercised here are **real MainNet blocks** captured from a
//! live Neo N3 node, not synthetic fixtures. Every assertion therefore compares
//! this implementation against bytes the C# reference implementation actually
//! produced on-chain.
//!
//! Refresh the vectors with `python scripts/generate-block-test-vectors.py`.

mod protocol_compliance;

#[cfg(test)]
mod tests {
    use super::protocol_compliance::test_vectors::block_validation as blockvec;
    use super::protocol_compliance::*;
    use neo_core::UInt256;
    use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
    use neo_core::network::p2p::payloads::Block;

    fn from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    fn to_hex(bytes: &[u8]) -> String {
        use std::fmt::Write;
        bytes.iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{:02x}", b);
            s
        })
    }

    fn parse_block(hex_str: &str) -> Block {
        let bytes = from_hex(hex_str);
        let mut reader = MemoryReader::new(&bytes);
        Block::deserialize(&mut reader).expect("block must deserialize")
    }

    #[test]
    fn test_harness_initialization() {
        let harness = test_harness::ProtocolTestHarness::new();
        assert_eq!(harness.test_vectors.len(), 0);
    }

    #[test]
    fn test_state_root_comparison_match() {
        let root1 = vec![1, 2, 3, 4];
        let root2 = vec![1, 2, 3, 4];
        let result = state_comparison::compare_state_roots(&root1, &root2);
        assert!(result.is_compliant());
    }

    #[test]
    fn test_state_root_comparison_mismatch() {
        let root1 = vec![1, 2, 3, 4];
        let root2 = vec![5, 6, 7, 8];
        let result = state_comparison::compare_state_roots(&root1, &root2);
        assert!(!result.is_compliant());
    }

    /// The checked-in fixture must be real MainNet data, not a placeholder.
    ///
    /// An earlier revision shipped an all-zero `mainnet_block_1000.hex` and an
    /// empty vector list; these assertions fail if that ever regresses.
    #[test]
    fn mainnet_vectors_are_real_chain_data() {
        let doc = blockvec::load_mainnet_vectors();
        assert_eq!(doc.network, "mainnet");
        assert_eq!(doc.magic, 860_833_102, "MainNet magic");
        assert!(
            doc.blocks.len() >= 20,
            "expected a broad vector set, got {}",
            doc.blocks.len()
        );
        assert_eq!(doc.block_count, doc.blocks.len());
        // Merkle-root computation is only meaningfully exercised by non-empty
        // blocks; an all-empty fixture would silently pass everything.
        let tx_bearing = doc.blocks.iter().filter(|b| b.tx_count > 0).count();
        assert!(
            tx_bearing >= 8,
            "expected many transaction-bearing blocks, got {tx_bearing}"
        );
        // At least one block must be large enough to build a deep merkle tree.
        let max_tx = doc.blocks.iter().map(|b| b.tx_count).max().unwrap_or(0);
        assert!(
            max_tx >= 256,
            "expected a deep merkle tree block, max {max_tx}"
        );
    }

    /// Deserialising every real MainNet block must reproduce the hash the
    /// network already agreed on.
    #[test]
    fn real_mainnet_blocks_hash_to_the_reported_value() {
        for v in blockvec::mainnet_block_vectors() {
            let mut block = parse_block(&v.block_hex);
            let expected = UInt256::parse(&v.hash).expect("valid hash in fixture");
            assert_eq!(
                block.hash(),
                expected,
                "block {} ({}) hash mismatch",
                v.height,
                v.note
            );
        }
    }

    /// Re-serialising must reproduce the exact bytes the C# node produced.
    /// This is the strictest wire-format check available.
    #[test]
    fn real_mainnet_blocks_round_trip_byte_for_byte() {
        for v in blockvec::mainnet_block_vectors() {
            let block = parse_block(&v.block_hex);
            let mut writer = BinaryWriter::new();
            block.serialize(&mut writer).expect("serialize");
            assert_eq!(
                to_hex(&writer.into_bytes()),
                v.block_hex,
                "block {} ({}) round-trip mismatch",
                v.height,
                v.note
            );
        }
    }

    #[test]
    fn real_mainnet_block_size_matches_csharp() {
        for v in blockvec::mainnet_block_vectors() {
            let block = parse_block(&v.block_hex);
            assert_eq!(
                Serializable::size(&block),
                v.size,
                "block {} ({}) size mismatch",
                v.height,
                v.note
            );
        }
    }

    #[test]
    fn real_mainnet_block_header_fields_match_csharp() {
        for v in blockvec::mainnet_block_vectors() {
            let block = parse_block(&v.block_hex);
            let ctx = format!("block {} ({})", v.height, v.note);

            assert_eq!(block.index(), v.height, "{ctx}: index");
            assert_eq!(block.timestamp(), v.time, "{ctx}: timestamp");
            assert_eq!(block.transactions.len(), v.tx_count, "{ctx}: tx count");

            let expected_root = UInt256::parse(&v.merkleroot).expect("valid merkleroot");
            assert_eq!(*block.merkle_root(), expected_root, "{ctx}: merkle root");

            let expected_prev =
                UInt256::parse(&v.previousblockhash).expect("valid previousblockhash");
            assert_eq!(*block.prev_hash(), expected_prev, "{ctx}: prev hash");
        }
    }

    /// A block whose header merkle root disagrees with its transactions must be
    /// rejected, exactly as C# `Block.DeserializeTransactions` does.
    #[test]
    fn block_with_tampered_merkle_root_is_rejected() {
        let v = blockvec::mainnet_block_vectors()
            .into_iter()
            .find(|b| b.tx_count > 0)
            .expect("a transaction-bearing vector");

        let mut bytes = from_hex(&v.block_hex);
        // The merkle root sits right after version (4) + prev hash (32).
        let root_offset = 4 + 32;
        bytes[root_offset] ^= 0xff;

        let mut reader = MemoryReader::new(&bytes);
        assert!(
            Block::deserialize(&mut reader).is_err(),
            "mismatched merkle root must be rejected"
        );
    }

    /// C# rejects duplicate transaction hashes during deserialization.
    #[test]
    fn block_with_duplicate_transactions_is_rejected() {
        let v = blockvec::mainnet_block_vectors()
            .into_iter()
            .find(|b| b.tx_count == 1)
            .expect("a single-transaction vector");

        let block = parse_block(&v.block_hex);
        let tx = block.transactions[0].clone();

        // Rebuild the block with the same transaction twice. The merkle root is
        // recomputed over the duplicated list so the failure can only come from
        // the duplicate-hash guard.
        let mut duplicated = Block::new();
        duplicated.header = block.header.clone();
        duplicated.transactions = vec![tx.clone(), tx];
        duplicated.rebuild_merkle_root();

        let mut writer = BinaryWriter::new();
        duplicated.serialize(&mut writer).expect("serialize");
        let bytes = writer.into_bytes();

        let mut reader = MemoryReader::new(&bytes);
        assert!(
            Block::deserialize(&mut reader).is_err(),
            "duplicate transactions must be rejected"
        );
    }
}
