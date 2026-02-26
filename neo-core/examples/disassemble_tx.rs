use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::LedgerContract;
use neo_core::UInt256;
use neo_vm::op_code::OpCode;
use neo_vm::Script;
use std::path::PathBuf;

fn format_ascii_preview(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for byte in bytes.iter().copied() {
        if byte.is_ascii_graphic() || byte == b' ' {
            out.push(byte as char);
        } else {
            out.push('.');
        }
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: disassemble_tx <db_path> <tx_hash> [strict=false]")?;
    let tx_hash_raw = args
        .next()
        .ok_or("usage: disassemble_tx <db_path> <tx_hash> [strict=false]")?;
    let strict = args
        .next()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false);

    let tx_hash = UInt256::parse(tx_hash_raw.trim_start_matches("0x"))?;

    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);
    let ledger = LedgerContract::new();

    let tx_state = ledger
        .get_transaction_state(&cache, &tx_hash)?
        .ok_or("transaction not found")?;
    let tx = tx_state.transaction().clone();
    let script_bytes = tx.script().to_vec();
    let script = Script::new(script_bytes.clone(), strict)?;

    println!(
        "tx={} block={} strict={} script_len={} sender={}",
        tx.hash(),
        tx_state.block_index(),
        strict,
        script_bytes.len(),
        tx.sender()
            .map(|sender| sender.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    );

    let mut offset = 0usize;
    while offset < script.len() {
        let instruction = script
            .get_instruction(offset)
            .map_err(|err| format!("failed to decode instruction at {offset}: {err}"))?;
        let opcode = instruction.opcode();
        let size = instruction.size();

        match opcode {
            OpCode::SYSCALL => {
                println!(
                    "offset={offset:>4} opcode={opcode:?} size={size} syscall=0x{:08x}",
                    instruction.token_u32()
                );
            }
            OpCode::CALLT => {
                println!(
                    "offset={offset:>4} opcode={opcode:?} size={size} token_id={}",
                    instruction.token_u16()
                );
            }
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4 => {
                let payload = instruction.operand();
                let preview_len = payload.len().min(48);
                println!(
                    "offset={offset:>4} opcode={opcode:?} size={size} data_len={} preview_hex=0x{} preview_ascii=\"{}\"",
                    payload.len(),
                    hex::encode(&payload[..preview_len]),
                    format_ascii_preview(&payload[..preview_len]),
                );
            }
            _ => {
                println!("offset={offset:>4} opcode={opcode:?} size={size}");
            }
        }

        if size == 0 {
            return Err(format!("decoded zero-size instruction at offset {offset}").into());
        }
        offset += size;
    }

    Ok(())
}
