//! Read-only MainNet transfer-script distribution profiler for official `.acc` packages.

use anyhow::{bail, Context, Result};
use clap::Parser;
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UInt160;
use neo_protocol::payloads::Block;
use neo_vm::fast_path::Nep17Parser;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    offline_package: PathBuf,
    #[arg(long)]
    start: u32,
    #[arg(long)]
    end: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.start > args.end {
        bail!("start must not exceed end");
    }
    let mut file = File::open(&args.offline_package)?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header)?;
    let package_start = u32::from_le_bytes(header[..4].try_into().unwrap());
    let count = u32::from_le_bytes(header[4..].try_into().unwrap());
    let package_end = package_start
        .checked_add(count)
        .and_then(|end| end.checked_sub(1))
        .context("invalid package range")?;
    if args.start < package_start || args.end > package_end {
        bail!("requested range is outside the package");
    }
    for height in package_start..args.start {
        skip_block(&mut file, height)?;
    }

    let mut transactions = 0u64;
    let mut standard_transfers = 0u64;
    let mut self_transfers = 0u64;
    let mut senders = HashSet::new();
    let mut recipients = HashSet::new();
    let mut contracts: HashMap<UInt160, u64> = HashMap::new();
    for height in args.start..=args.end {
        let raw = read_block(&mut file, height)?;
        let mut reader = MemoryReader::new(&raw);
        let block = <Block as Serializable>::deserialize(&mut reader)?;
        if reader.remaining() != 0 || block.index() != height {
            bail!("invalid block at {height}");
        }
        for transaction in &block.transactions {
            transactions += 1;
            if let Ok(params) = Nep17Parser::parse_transfer_script(transaction.script()) {
                standard_transfers += 1;
                self_transfers += u64::from(params.from == params.to);
                senders.insert(params.from);
                recipients.insert(params.to);
                let hash = UInt160::from_bytes(&params.contract_hash)?;
                *contracts.entry(hash).or_default() += 1;
            }
        }
    }
    let mut contracts: Vec<_> = contracts.into_iter().collect();
    contracts.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    let contract_rows: Vec<_> = contracts
        .iter()
        .map(|(hash, calls)| json!({"contract_hash": hash.to_string(), "transactions": calls}))
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "range": [args.start, args.end],
            "blocks": args.end - args.start + 1,
            "transactions": transactions,
            "standard_nep17_transfer_scripts": standard_transfers,
            "standard_transfer_percent": if transactions == 0 { 0.0 } else { standard_transfers as f64 * 100.0 / transactions as f64 },
            "self_transfers": self_transfers,
            "unique_senders": senders.len(),
            "unique_recipients": recipients.len(),
            "unique_called_contracts": contracts.len(),
            "contracts": contract_rows,
        }))?
    );
    Ok(())
}

fn skip_block(file: &mut File, height: u32) -> Result<()> {
    let mut length = [0u8; 4];
    file.read_exact(&mut length)
        .with_context(|| format!("missing block length at {height}"))?;
    let length = i32::from_le_bytes(length);
    if length <= 0 {
        bail!("invalid block length at {height}");
    }
    file.seek(SeekFrom::Current(i64::from(length)))?;
    Ok(())
}

fn read_block(file: &mut File, height: u32) -> Result<Vec<u8>> {
    let mut length = [0u8; 4];
    file.read_exact(&mut length)?;
    let length = i32::from_le_bytes(length);
    if length <= 0 || length > 64 * 1024 * 1024 {
        bail!("invalid block length at {height}");
    }
    let mut raw = vec![0u8; length as usize];
    file.read_exact(&mut raw)?;
    Ok(raw)
}
