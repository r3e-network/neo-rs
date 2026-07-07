//! Remote-ledger RPC result normalization.

use neo_primitives::UInt256;
use serde_json::Value;

pub(super) fn parse_remote_mempool_hashes(value: Value) -> anyhow::Result<Vec<UInt256>> {
    let hashes = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value.get("verified").and_then(Value::as_array) {
        array
    } else {
        anyhow::bail!("remote ledger RPC getrawmempool returned non-array result");
    };
    hashes
        .iter()
        .map(|value| {
            let hash = value.as_str().ok_or_else(|| {
                anyhow::anyhow!("remote ledger RPC getrawmempool returned non-string hash")
            })?;
            UInt256::parse(hash).map_err(|err| anyhow::anyhow!("{err}"))
        })
        .collect()
}
