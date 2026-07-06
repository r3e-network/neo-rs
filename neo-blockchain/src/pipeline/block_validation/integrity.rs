use neo_crypto::MerkleTree;
use neo_payloads::Block;
use neo_primitives::UInt256;

use super::{BlockValidationError, BlockValidator};

impl BlockValidator {
    /// Validates the stateless block-integrity checks shared by live inventory
    /// import and the reusable [`neo_runtime::BlockImport::check`] boundary.
    ///
    /// This intentionally mirrors the structural subset of C# `Block.Verify`
    /// used by `Blockchain.OnNewBlock`: block version, transaction merkle root,
    /// and duplicate transaction hashes. It does **not** enforce
    /// `MaxTransactionsPerBlock`; Neo C# treats that as a dBFT block-production
    /// limit rather than a peer block validity rule.
    pub fn validate_import_integrity(block: &Block) -> Result<(), BlockValidationError> {
        Self::validate_block_version(block.version())?;
        let tx_hashes = block.transaction_hashes().map_err(|err| {
            BlockValidationError::HeaderValidationFailed {
                reason: format!("failed to hash block transactions: {err}"),
            }
        })?;
        Self::validate_merkle_root(block.header.merkle_root(), &tx_hashes)?;
        Self::validate_no_duplicate_transactions(&tx_hashes)
    }

    /// Validates merkle root integrity against transaction hashes.
    ///
    /// Takes pre-computed transaction hashes so this function has no
    /// dependency on the concrete `Transaction` type. The caller is
    /// responsible for computing the hashes from whatever transaction
    /// representation they hold.
    ///
    /// # Arguments
    /// * `merkle_root` - The expected merkle root from the header
    /// * `tx_hashes` - The transaction hashes in canonical block order
    ///
    /// # Returns
    /// * `Ok(())` if merkle root matches
    /// * `Err(BlockValidationError)` if merkle root is invalid
    pub fn validate_merkle_root(
        merkle_root: &UInt256,
        tx_hashes: &[UInt256],
    ) -> Result<(), BlockValidationError> {
        // Empty block should have zero merkle root
        if tx_hashes.is_empty() {
            if *merkle_root != UInt256::default() {
                return Err(BlockValidationError::InvalidMerkleRoot {
                    expected: *merkle_root,
                    computed: UInt256::default(),
                });
            }
            return Ok(());
        }

        // Compute merkle root from the pre-computed transaction hashes.
        match MerkleTree::compute_root(tx_hashes) {
            Some(computed_root) => {
                if computed_root != *merkle_root {
                    return Err(BlockValidationError::InvalidMerkleRoot {
                        expected: *merkle_root,
                        computed: computed_root,
                    });
                }
                Ok(())
            }
            None => Err(BlockValidationError::InvalidMerkleRoot {
                expected: *merkle_root,
                computed: UInt256::default(),
            }),
        }
    }

    /// Validates there are no duplicate transaction hashes in the block.
    ///
    /// Takes pre-computed transaction hashes so this function has no
    /// dependency on the concrete `Transaction` type. The caller is
    /// responsible for computing the hashes from whatever transaction
    /// representation they hold.
    ///
    /// # Arguments
    /// * `tx_hashes` - The transaction hashes to check for duplicates
    ///
    /// # Returns
    /// * `Ok(())` if no duplicates found
    /// * `Err(BlockValidationError)` if duplicates exist
    pub fn validate_no_duplicate_transactions(
        tx_hashes: &[UInt256],
    ) -> Result<(), BlockValidationError> {
        let mut seen = std::collections::HashSet::with_capacity(tx_hashes.len());
        for hash in tx_hashes {
            if !seen.insert(*hash) {
                return Err(BlockValidationError::DuplicateTransactions);
            }
        }
        Ok(())
    }
}
