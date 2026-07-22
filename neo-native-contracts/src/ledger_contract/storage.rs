use crate::keys::{
    prefixed_hash256_hash160_key, prefixed_hash256_key, prefixed_key, prefixed_u32_be_key,
};
use neo_primitives::{UInt160, UInt256};
use neo_storage::StorageKey;

use super::LedgerContract;

/// Storage prefix for the per-block-index -> block-hash index.
pub const PREFIX_BLOCK_HASH: u8 = 9;
/// Storage prefix for the trimmed-block payload.
pub const PREFIX_BLOCK: u8 = 5;
/// Storage prefix for the per-transaction state record.
pub const PREFIX_TRANSACTION: u8 = 11;
/// Storage prefix for the current-block (hash, index) pointer.
pub const PREFIX_CURRENT_BLOCK: u8 = 12;

impl LedgerContract {
    /// Exact native-contract storage key holding the current `(hash, index)`
    /// Ledger pointer.
    ///
    /// Recovery and offline migration tooling uses this owned key to bind an
    /// atomic storage guard to the same row decoded by
    /// [`Self::optional_current_tip`](super::LedgerContract::optional_current_tip).
    #[inline]
    pub fn current_block_storage_key() -> StorageKey {
        prefixed_key(Self::ID, PREFIX_CURRENT_BLOCK, &[])
    }

    /// C# `CreateStorageKey(Prefix_BlockHash, uint bigEndianKey)`
    /// (NativeContract.cs:403 -> `KeyBuilder.AddBigEndian(uint)`): the block
    /// index is encoded big-endian so the index keys sort in block order.
    #[inline]
    pub(crate) fn block_hash_storage_key(index: u32) -> StorageKey {
        prefixed_u32_be_key(Self::ID, PREFIX_BLOCK_HASH, index)
    }

    #[inline]
    pub(crate) fn transaction_storage_key(hash: &UInt256) -> StorageKey {
        prefixed_hash256_key(Self::ID, PREFIX_TRANSACTION, hash)
    }

    /// C# `CreateStorageKey(Prefix_Transaction, UInt256 hash, UInt160 signer)`
    /// — the per-signer conflict-stub key.
    #[inline]
    pub(crate) fn conflict_signer_storage_key(hash: &UInt256, signer: &UInt160) -> StorageKey {
        prefixed_hash256_hash160_key(Self::ID, PREFIX_TRANSACTION, hash, signer)
    }

    #[inline]
    pub(crate) fn block_storage_key(hash: &UInt256) -> StorageKey {
        prefixed_hash256_key(Self::ID, PREFIX_BLOCK, hash)
    }
}
