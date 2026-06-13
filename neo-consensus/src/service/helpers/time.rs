use std::time::{SystemTime, UNIX_EPOCH};

/// Gets the current timestamp in milliseconds
pub(in crate::service) fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// C# `ConsensusContext.MakePrepareRequest` sets
/// `Block.Header.Timestamp = Math.Max(now, PrevHeader.Timestamp + 1)`.
pub(in crate::service) fn prepare_request_timestamp(
    now_ms: u64,
    previous_block_timestamp: u64,
) -> u64 {
    now_ms.max(previous_block_timestamp.saturating_add(1))
}

pub(in crate::service) fn generate_nonce() -> u64 {
    use rand::RngCore;

    let mut bytes = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    u64::from_le_bytes(bytes)
}
