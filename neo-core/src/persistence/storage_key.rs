//! StorageKey re-export from neo-storage.
//!
//! The `StorageKey` struct is now defined in [`neo_storage`] as the single source of truth.
//! This module re-exports it for backward compatibility.

pub use neo_storage::StorageKey;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UInt160, UInt256};

    #[test]
    fn storage_key_basic() {
        let key = StorageKey::new(-1, vec![0x01, 0x02, 0x03]);
        assert_eq!(key.id(), -1);
        assert_eq!(key.key(), &[0x01, 0x02, 0x03]);
    }

    #[test]
    fn storage_key_create() {
        let key = StorageKey::create(-4, 0x05);
        assert_eq!(key.id(), -4);
        assert_eq!(key.key(), &[0x05]);
    }

    #[test]
    fn storage_key_create_with_uint160() {
        let hash = UInt160::zero();
        let key = StorageKey::create_with_uint160(-1, 0x14, &hash);
        assert_eq!(key.id(), -1);
        assert_eq!(key.key().len(), 21); // 1 prefix + 20 bytes hash
    }

    #[test]
    fn storage_key_create_with_uint256() {
        let hash = UInt256::zero();
        let key = StorageKey::create_with_uint256(-2, 0x15, &hash);
        assert_eq!(key.id(), -2);
        assert_eq!(key.key().len(), 33); // 1 prefix + 32 bytes hash
    }

    #[test]
    fn storage_key_ordering() {
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);
        let key3 = StorageKey::new(0, vec![0x01]);

        assert!(key1 < key2);
        assert!(key1 < key3);
    }

    #[test]
    fn storage_key_to_array() {
        let key = StorageKey::new(-1, vec![0xAA, 0xBB]);
        let array = key.to_array();
        assert_eq!(&array[..4], &(-1i32).to_le_bytes());
        assert_eq!(&array[4..], &[0xAA, 0xBB]);
    }

    #[test]
    fn storage_key_from_bytes() {
        let bytes = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x02];
        let key = StorageKey::from_bytes(&bytes);
        assert_eq!(key.id(), -1);
        assert_eq!(key.key(), &[0x01, 0x02]);
    }

    #[test]
    fn storage_key_get_hash_code() {
        let key = StorageKey::new(-1, vec![0x14, 0xAA, 0xBB]);
        let hash1 = key.get_hash_code();
        let hash2 = key.get_hash_code();
        assert_eq!(hash1, hash2);
    }
}
