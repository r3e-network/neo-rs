//! Rust 状态处理器
//!
//! 处理下载的区块并提取每个区块后的状态根。

#![allow(dead_code)] // JSON fixture fields are retained for schema compatibility.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct BlocksFile {
    network: String,
    start_height: u32,
    end_height: u32,
    block_count: usize,
    blocks: Vec<BlockEntry>,
}

#[derive(Debug, Deserialize)]
struct BlockEntry {
    height: u32,
    hash: String,
    size: usize,
    time: u64,
    tx_count: usize,
    block_data: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct StateRoot {
    height: u32,
    block_hash: String,
    state_root_hash: String,
    timestamp: u64,
}

#[derive(Debug, Serialize)]
struct StateRootsOutput {
    network: String,
    start_height: u32,
    end_height: u32,
    state_roots: Vec<StateRoot>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!(
            "Usage: {} <input_blocks.json> <output_state_roots.json>",
            args[0]
        );
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[1]);
    let output_path = PathBuf::from(&args[2]);

    println!("Loading blocks from {}...", input_path.display());
    let blocks_json = fs::read_to_string(&input_path)?;
    let blocks_file: BlocksFile = serde_json::from_str(&blocks_json)?;

    println!("Processing {} blocks...", blocks_file.block_count);

    let mut state_roots = Vec::new();

    for (i, block_entry) in blocks_file.blocks.iter().enumerate() {
        if i % 100 == 0 {
            println!(
                "Progress: {}/{} blocks processed",
                i, blocks_file.block_count
            );
        }

        // TODO: 实际的区块处理逻辑
        // 现在先使用占位符
        let state_root = StateRoot {
            height: block_entry.height,
            block_hash: block_entry.hash.clone(),
            state_root_hash: format!("0x{:064x}", block_entry.height), // 占位符
            timestamp: block_entry.time,
        };

        state_roots.push(state_root);
    }

    println!("Processed {} blocks", state_roots.len());

    // 保存状态根
    let output = StateRootsOutput {
        network: blocks_file.network,
        start_height: blocks_file.start_height,
        end_height: blocks_file.end_height,
        state_roots,
    };

    let output_json = serde_json::to_string_pretty(&output)?;
    fs::write(&output_path, output_json)?;

    println!("✅ State roots saved to {}", output_path.display());

    Ok(())
}
