//! Replays downloaded RPC blocks through the canonical neo-rs execution pipeline.

use anyhow::{bail, Context, Result};
use neo_json::JToken;
use neo_primitives::{BlockLike, UInt256};
use neo_protocol::ProtocolSettings;
use neo_rpc::client::models::RpcBlock;
use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use neo_storage::persistence::store_provider::StoreProvider;
use neo_validator::executor::{BlockExecutor, NeoExecutor};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlocksFile {
    network: String,
    #[serde(rename = "rpc_url")]
    _rpc_url: String,
    #[serde(rename = "downloaded_at")]
    _downloaded_at: String,
    start_height: u32,
    end_height: u32,
    block_count: usize,
    blocks: Vec<BlockEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockEntry {
    height: u32,
    hash: String,
    size: usize,
    time: u64,
    tx_count: usize,
    block_data: serde_json::Value,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct StateRoot {
    height: u32,
    block_hash: String,
    state_root_hash: String,
    timestamp: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct StateRootsOutput {
    network: String,
    start_height: u32,
    end_height: u32,
    state_roots: Vec<StateRoot>,
}

#[derive(Debug, Serialize)]
struct NativeStorageEntry {
    id: i32,
    key: String,
    value: String,
}

fn hex_string(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        },
    )
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if !(3..=4).contains(&args.len()) {
        bail!(
            "usage: {} <input_blocks.json> <output_state_roots.json> [output_native_storage.json]",
            args[0]
        );
    }
    let input_path = PathBuf::from(&args[1]);
    let output_path = PathBuf::from(&args[2]);
    let blocks_json = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;
    let blocks_file: BlocksFile = serde_json::from_str(&blocks_json)
        .with_context(|| format!("invalid block fixture {}", input_path.display()))?;
    let provider: Arc<dyn StoreProvider> = Arc::new(MemoryStoreProvider::new());
    let executor = NeoExecutor::open_with_provider(
        protocol_settings(&blocks_file.network)?,
        provider,
        Path::new("state-root-diff-memory"),
    )?;
    let output = process_blocks(&blocks_file, &executor)?;
    if let Some(storage_path) = args.get(3).map(PathBuf::from) {
        let storage = executor
            .native_contract_storage()
            .into_iter()
            .map(|(id, key, value)| NativeStorageEntry {
                id,
                key: hex_string(&key),
                value: hex_string(&value),
            })
            .collect::<Vec<_>>();
        let storage_json = serde_json::to_string_pretty(&storage)?;
        fs::write(&storage_path, storage_json)
            .with_context(|| format!("failed to write {}", storage_path.display()))?;
    }
    executor.shutdown().await?;
    let output_json = serde_json::to_string_pretty(&output)?;
    fs::write(&output_path, output_json)
        .with_context(|| format!("failed to write {}", output_path.display()))?;
    println!(
        "Executed {} blocks and saved real state roots to {}",
        output.state_roots.len(),
        output_path.display()
    );
    Ok(())
}

fn protocol_settings(network: &str) -> Result<ProtocolSettings> {
    match network.to_ascii_lowercase().as_str() {
        "mainnet" => Ok(ProtocolSettings::mainnet()),
        "testnet" => Ok(ProtocolSettings::testnet()),
        "private" | "privatenet" => Ok(ProtocolSettings::default()),
        _ => bail!("unsupported network {network}; expected mainnet, testnet or private"),
    }
}

fn process_blocks(file: &BlocksFile, executor: &NeoExecutor) -> Result<StateRootsOutput> {
    let settings = protocol_settings(&file.network)?;
    if file.block_count != file.blocks.len() {
        bail!(
            "block_count {} does not match {} entries",
            file.block_count,
            file.blocks.len()
        );
    }
    if file.blocks.is_empty()
        || file.start_height != 0
        || file.end_height.checked_add(1) != u32::try_from(file.block_count).ok()
    {
        bail!("input must contain one contiguous genesis-to-tip block range");
    }

    let mut state_roots = Vec::with_capacity(file.blocks.len());
    for (position, entry) in file.blocks.iter().enumerate() {
        let expected_height = u32::try_from(position).context("block height overflow")?;
        if entry.height != expected_height {
            bail!(
                "non-contiguous input at position {position}: height {}, expected {expected_height}",
                entry.height
            );
        }
        let block = parse_rpc_block(&entry.block_data, &settings)?;
        if block.index() != entry.height
            || block.timestamp() != entry.time
            || block.size() != entry.size
            || block.transactions.len() != entry.tx_count
        {
            bail!("block envelope metadata differs from RPC block at {expected_height}");
        }
        let expected_hash = UInt256::parse(&entry.hash).context("invalid envelope block hash")?;
        let outcome = executor
            .execute(block)
            .with_context(|| format!("failed to execute block {expected_height}"))?;
        if outcome.block_hash != expected_hash {
            bail!("executed block hash differs from envelope at {expected_height}");
        }
        if outcome.state_root == UInt256::zero() {
            bail!("executor returned the invalid zero state root at {expected_height}");
        }
        state_roots.push(StateRoot {
            height: entry.height,
            block_hash: outcome.block_hash.to_string(),
            state_root_hash: outcome.state_root.to_string(),
            timestamp: entry.time,
        });
    }
    Ok(StateRootsOutput {
        network: file.network.clone(),
        start_height: file.start_height,
        end_height: file.end_height,
        state_roots,
    })
}

fn parse_rpc_block(
    value: &serde_json::Value,
    settings: &ProtocolSettings,
) -> Result<neo_ledger::Block> {
    let text = serde_json::to_string(value)?;
    let token = JToken::parse(&text, 128).context("invalid RPC block JSON")?;
    let object = token
        .as_object()
        .context("RPC block data must be a JSON object")?;
    Ok(RpcBlock::from_json(object, settings)
        .map_err(anyhow::Error::msg)?
        .block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn genesis_fixture_produces_the_real_persisted_root() {
        let mut file: BlocksFile =
            serde_json::from_str(include_str!("data/test_blocks.json")).expect("genesis fixture");
        file.blocks.truncate(1);
        file.block_count = 1;
        file.end_height = 0;
        let provider: Arc<dyn StoreProvider> = Arc::new(MemoryStoreProvider::new());
        let executor = NeoExecutor::open_with_provider(
            ProtocolSettings::mainnet(),
            provider,
            Path::new("state-root-diff-test-memory"),
        )
        .expect("open executor");
        let output = process_blocks(&file, &executor).expect("execute genesis");
        executor.shutdown().await.expect("shutdown");
        assert_eq!(output.state_roots.len(), 1);
        assert_eq!(
            output.state_roots[0].state_root_hash,
            "0x58a5157b7e99eeabf631291f1747ec8eb12ab89461cda888492b17a301de81e8"
        );
    }
}
