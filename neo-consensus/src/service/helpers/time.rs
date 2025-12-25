use std::time::{SystemTime, UNIX_EPOCH};

/// Gets the current timestamp in milliseconds
pub(in crate::service) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub(in crate::service) fn generate_nonce() -> u64 {
    use rand::RngCore;

    let mut bytes = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    u64::from_le_bytes(bytes)
}
