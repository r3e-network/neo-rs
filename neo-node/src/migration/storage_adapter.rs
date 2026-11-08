//! Storage format adapters for converting between implementations.

use anyhow::Result;

/// neo-cli storage key prefixes (same as used in neo-rs)
pub mod neo_cli_keys {
    pub const DATA_BLOCK: u8 = 0x01;           // Block data
    pub const DATA_TRANSACTION: u8 = 0x02;     // Transaction data
    pub const ST_STORAGE: u8 = 0x40;           // Contract storage
    pub const IX_HEADER_HASH_LIST: u8 = 0x50;  // Block header index
    pub const SYS_CURRENT_BLOCK: u8 = 0x05;    // Current block hash + height
    pub const SYS_CURRENT_HEADER: u8 = 0x06;   // Current header hash + height
    pub const SYS_VERSION: u8 = 0x00;          // Storage version
}

/// neo-go storage bucket names
pub mod neo_go_buckets {
    pub const BLOCK_BUCKET: &[u8] = b"Block";
    pub const TX_BUCKET: &[u8] = b"Transaction";
    pub const STATE_BUCKET: &[u8] = b"ST";
}

/// Convert neo-go key format to neo-cli/neo-rs format
pub fn convert_neo_go_key_to_neo_rs(bucket: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();

    if bucket == neo_go_buckets::BLOCK_BUCKET {
        result.push(neo_cli_keys::DATA_BLOCK);
        result.extend_from_slice(key);
    } else if bucket == neo_go_buckets::TX_BUCKET {
        result.push(neo_cli_keys::DATA_TRANSACTION);
        result.extend_from_slice(key);
    } else if bucket == neo_go_buckets::STATE_BUCKET {
        result.push(neo_cli_keys::ST_STORAGE);
        result.extend_from_slice(key);
    } else {
        // Unknown bucket, pass through
        result.extend_from_slice(key);
    }

    Ok(result)
}

/// Verify that a key/value pair is valid after conversion
pub fn verify_converted_entry(key: &[u8], value: &[u8]) -> bool {
    // Basic sanity checks
    if key.is_empty() || value.is_empty() {
        return false;
    }

    // Check key prefix is valid
    matches!(key[0],
        neo_cli_keys::DATA_BLOCK |
        neo_cli_keys::DATA_TRANSACTION |
        neo_cli_keys::ST_STORAGE |
        neo_cli_keys::IX_HEADER_HASH_LIST |
        neo_cli_keys::SYS_CURRENT_BLOCK |
        neo_cli_keys::SYS_CURRENT_HEADER |
        neo_cli_keys::SYS_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_neo_go_block_key() {
        let bucket = neo_go_buckets::BLOCK_BUCKET;
        let key = vec![1, 2, 3, 4];

        let result = convert_neo_go_key_to_neo_rs(bucket, &key).unwrap();

        assert_eq!(result[0], neo_cli_keys::DATA_BLOCK);
        assert_eq!(&result[1..], &key[..]);
    }

    #[test]
    fn test_verify_converted_entry() {
        let key = vec![neo_cli_keys::DATA_BLOCK, 1, 2, 3];
        let value = vec![1, 2, 3, 4, 5];

        assert!(verify_converted_entry(&key, &value));

        // Invalid: empty key
        assert!(!verify_converted_entry(&[], &value));

        // Invalid: empty value
        assert!(!verify_converted_entry(&key, &[]));
    }
}
