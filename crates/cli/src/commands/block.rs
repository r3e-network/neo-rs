use super::CommandResult;
use crate::{console::percent::ConsolePercent, console_service::ConsoleHelper};
use akka::{Actor, ActorContext, ActorResult, Props};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::{Local, TimeZone};
use hex;
use neo_core::{
    ledger::blockchain::{BlockchainCommand, Import, ImportCompleted},
    neo_io::{BinaryWriter, MemoryReader, Serializable},
    neo_system::NeoSystem,
    network::p2p::message::PAYLOAD_MAX_SIZE,
    network::p2p::payloads::block::Block as NeoBlock,
    smart_contract::native::NeoToken,
    UInt256,
};
use std::{
    any::Any,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;
use zip::read::ZipArchive;

/// Commands related to block export/import (see `Neo.CLI/CLI/MainService.Block.cs`).
pub struct BlockCommands {
    system: Arc<NeoSystem>,
}

impl BlockCommands {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    /// Imports blocks from the default bootstrap files (`chain*.acc`).
    pub fn import_default_chain_files(&self, verify: bool) -> CommandResult {
        self.import_plain_file(Path::new("chain.acc"), false, verify, true)?;
        self.import_zip_file(Path::new("chain.acc.zip"), "chain.acc", false, verify, true)?;

        let mut archives = discover_chain_archives()?;
        archives.sort_by_key(|entry| entry.start);
        let mut current_height = self.system.current_block_index();

        for archive in archives {
            if archive.start > current_height.saturating_add(1) {
                break;
            }

            if archive.compressed {
                self.import_zip_file(&archive.path, &archive.entry_name, true, verify, false)?;
            } else {
                self.import_plain_file(&archive.path, true, verify, false)?;
            }
            current_height = self.system.current_block_index();
        }

        Ok(())
    }

    /// Displays block details (mirrors `OnShowBlockCommand`).
    pub fn show_block(&self, index_or_hash: &str) -> CommandResult {
        let (hash, block) = if let Ok(index) = index_or_hash.parse::<u32>() {
            let hash = self
                .system
                .block_hash_at(index)
                .ok_or_else(|| anyhow!("Enter a valid block index or hash."))?;
            let block = self
                .system
                .context()
                .try_get_block(&hash)
                .ok_or_else(|| anyhow!("Block {} doesn't exist.", index_or_hash))?;
            (hash, block)
        } else if let Ok(hash) = index_or_hash.parse::<UInt256>() {
            let block = self
                .system
                .context()
                .try_get_block(&hash)
                .ok_or_else(|| anyhow!("Block {} doesn't exist.", index_or_hash))?;
            (hash, block)
        } else {
            bail!("Enter a valid block index or hash.");
        };

        let dt = Local
            .timestamp_millis_opt(block.timestamp() as i64)
            .single()
            .unwrap_or_else(|| {
                Local
                    .timestamp_millis_opt(0)
                    .single()
                    .unwrap_or_else(|| Local.timestamp_millis_opt(0).earliest().unwrap())
            });

        ConsoleHelper::info(["", "-------------", "Block", "-------------"]);
        ConsoleHelper::info([""]);
        ConsoleHelper::info(["", "      Timestamp: ", &dt.to_string()]);
        ConsoleHelper::info(["", "          Index: ", &block.index().to_string()]);
        ConsoleHelper::info(["", "           Hash: ", &hash.to_string()]);
        ConsoleHelper::info(["", "          Nonce: ", &block.nonce().to_string()]);
        ConsoleHelper::info(["", "     MerkleRoot: ", &block.merkle_root().to_string()]);
        ConsoleHelper::info(["", "       PrevHash: ", &block.prev_hash().to_string()]);
        ConsoleHelper::info(["", "  NextConsensus: ", &block.next_consensus().to_string()]);
        ConsoleHelper::info(["", "   PrimaryIndex: ", &block.primary_index().to_string()]);
        let primary_pubkey = {
            let snapshot = self.system.context().store_cache();
            let settings = self.system.settings();
            let committee = NeoToken::new()
                .committee_from_snapshot(&snapshot)
                .filter(|committee| !committee.is_empty())
                .unwrap_or_else(|| settings.standby_committee.clone());
            committee
                .get(block.primary_index() as usize)
                .map(|point| hex::encode(point.encoded()))
                .unwrap_or_else(|| "<unknown>".to_string())
        };
        ConsoleHelper::info(["", "  PrimaryPubKey: ", &primary_pubkey]);
        ConsoleHelper::info(["", "        Version: ", &block.version().to_string()]);
        ConsoleHelper::info([
            "",
            "           Size: ",
            &format!("{} Byte(s)", block.size()),
        ]);
        ConsoleHelper::info([""]);

        ConsoleHelper::info(["", "-------------", "Witness", "-------------"]);
        ConsoleHelper::info([""]);
        let witness = block.witness();
        ConsoleHelper::info([
            "",
            "    Invocation Script: ",
            &BASE64.encode(witness.invocation_script()),
        ]);
        ConsoleHelper::info([
            "",
            "  Verification Script: ",
            &BASE64.encode(witness.verification_script()),
        ]);
        ConsoleHelper::info([
            "",
            "           ScriptHash: ",
            &witness.script_hash().to_string(),
        ]);
        ConsoleHelper::info([
            "",
            "                 Size: ",
            &format!("{} Byte(s)", witness.size()),
        ]);
        ConsoleHelper::info([""]);

        ConsoleHelper::info(["", "-------------", "Transactions", "-------------"]);
        ConsoleHelper::info([""]);

        if block.transactions.is_empty() {
            ConsoleHelper::info(["", "  No Transaction(s)"]);
        } else {
            for tx in &block.transactions {
                ConsoleHelper::info(["  ", &tx.hash().to_string()]);
            }
        }
        ConsoleHelper::info([""]);
        ConsoleHelper::info(["", "--------------------------------------"]);
        Ok(())
    }

    /// Exports blocks to an `.acc` file (mirrors `OnExportBlocksStartCountCommand`).
    pub fn export_blocks(&self, start: u32, count: u32, path: impl AsRef<Path>) -> CommandResult {
        let height = self.system.current_block_index();
        if start > height {
            bail!("invalid start height (current height: {height})");
        }

        let available = height.saturating_sub(start).saturating_add(1);
        let actual_count = available.min(count);
        if actual_count == 0 {
            bail!("block count must be greater than zero");
        }
        let end = start.saturating_add(actual_count - 1);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path.as_ref())
            .with_context(|| format!("failed to open {}", path.as_ref().display()))?;

        if file.metadata()?.len() > 0 {
            bail!(
                "destination {} already contains data; remove it first",
                path.as_ref().display()
            );
        }

        file.write_all(&start.to_le_bytes())?;
        file.write_all(&actual_count.to_le_bytes())?;
        file.seek(SeekFrom::End(0))?;

        ConsoleHelper::info(["", &format!("Export block from {start} to {end}")]);

        let mut percent = ConsolePercent::new(start as u64, end as u64);
        for index in start..=end {
            let block = self.fetch_block(index)?;
            let bytes = serialize_block(&block)?;
            let size = i32::try_from(bytes.len())
                .map_err(|_| anyhow!("block {} exceeds maximum export size", index))?;
            file.write_all(&size.to_le_bytes())?;
            file.write_all(&bytes)?;
            percent.set_value(index as u64);
        }

        Ok(())
    }

    fn fetch_block(&self, index: u32) -> Result<NeoBlock> {
        let hash = self
            .system
            .block_hash_at(index)
            .ok_or_else(|| anyhow!("unable to resolve hash for block {}", index))?;
        self.system
            .context()
            .try_get_block(&hash)
            .ok_or_else(|| anyhow!("block {} is not available in the local store", index))
    }

    fn import_plain_file(
        &self,
        path: &Path,
        read_start: bool,
        verify: bool,
        silent: bool,
    ) -> CommandResult {
        if !path.exists() {
            if silent {
                return Ok(());
            }
            bail!("file {} does not exist", path.display());
        }

        ConsoleHelper::info(["", &format!("Importing blocks from {}", path.display())]);

        let mut file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        self.import_from_reader(&mut file, read_start, verify)
    }

    fn import_zip_file(
        &self,
        path: &Path,
        entry_name: &str,
        read_start: bool,
        verify: bool,
        silent: bool,
    ) -> CommandResult {
        if !path.exists() {
            if silent {
                return Ok(());
            }
            bail!("file {} does not exist", path.display());
        }

        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut archive = ZipArchive::new(file)
            .with_context(|| format!("failed to read zip {}", path.display()))?;
        let mut entry = archive
            .by_name(entry_name)
            .with_context(|| format!("zip entry '{}' missing in {}", entry_name, path.display()))?;
        ConsoleHelper::info([
            "",
            &format!("Importing blocks from {}/{}", path.display(), entry_name),
        ]);
        self.import_from_reader(&mut entry, read_start, verify)
    }

    fn import_from_reader<R: Read>(
        &self,
        reader: &mut R,
        read_start: bool,
        verify: bool,
    ) -> CommandResult {
        let mut start_index = if read_start { read_u32(reader)? } else { 0 };
        let count = read_u32(reader)?;
        if count == 0 {
            return Ok(());
        }

        let end_index = start_index.saturating_add(count.saturating_sub(1));
        let current_height = self.system.current_block_index();
        if end_index <= current_height {
            return Ok(());
        }
        ConsoleHelper::info([
            "",
            &format!("Import block from {start_index} to {end_index}"),
        ]);

        let mut percent = ConsolePercent::new(start_index as u64, end_index as u64);
        let mut current_height = self.system.current_block_index();
        let mut batch = Vec::with_capacity(10);

        while start_index <= end_index {
            let size = read_i32(reader)?;
            if size <= 0 {
                bail!("invalid block size {size} at height {start_index}");
            }
            if size as usize > PAYLOAD_MAX_SIZE {
                bail!(
                    "block at height {} has size {} bytes, exceeding maximum {} bytes",
                    start_index,
                    size,
                    PAYLOAD_MAX_SIZE
                );
            }

            let mut buffer = vec![0u8; size as usize];
            reader.read_exact(&mut buffer)?;

            if start_index > current_height {
                let block = deserialize_block(&buffer)?;
                batch.push(block);
                if batch.len() == 10 {
                    self.submit_import(std::mem::take(&mut batch), verify)?;
                    current_height = self.system.current_block_index();
                }
            }

            start_index = start_index.saturating_add(1);
            percent.set_value(start_index as u64);
        }

        if !batch.is_empty() {
            self.submit_import(batch, verify)?;
        }

        Ok(())
    }

    fn submit_import(&self, blocks: Vec<NeoBlock>, verify: bool) -> CommandResult {
        if blocks.is_empty() {
            return Ok(());
        }

        let command = BlockchainCommand::Import(Import { blocks, verify });
        let (tx, rx) = oneshot::channel();
        let completion = Arc::new(Mutex::new(Some(tx)));
        let props = {
            let completion = completion.clone();
            Props::new(move || ImportResponder {
                completion: completion.clone(),
            })
        };
        let actor_system = self.system.actor_system();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let responder = actor_system
            .actor_of(props, format!("cli-import-{unique}"))
            .map_err(|err| anyhow!("failed to spawn import responder: {}", err))?;

        self.system
            .blockchain_actor()
            .tell_from(command, Some(responder.clone()))
            .map_err(|err| anyhow!("failed to submit import batch: {}", err))?;

        rx.blocking_recv()
            .map_err(|_| anyhow!("block import cancelled"))?;
        let _ = actor_system.stop(&responder);
        Ok(())
    }
}

fn serialize_block(block: &NeoBlock) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    block
        .serialize(&mut writer)
        .map_err(|err| anyhow!("failed to serialize block: {}", err))?;
    Ok(writer.into_bytes())
}

fn deserialize_block(bytes: &[u8]) -> Result<NeoBlock> {
    NeoBlock::deserialize(&mut MemoryReader::new(bytes))
        .map_err(|err| anyhow!("failed to deserialize block: {}", err))
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_i32<R: Read>(reader: &mut R) -> Result<i32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(i32::from_le_bytes(buf))
}

struct ChainArchive {
    path: PathBuf,
    entry_name: String,
    start: u32,
    compressed: bool,
}

fn discover_chain_archives() -> Result<Vec<ChainArchive>> {
    let mut archives = Vec::new();
    for entry in fs::read_dir(".")? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let file_name = name.to_string();

        if let Some(start) = parse_chain_acc_start(&file_name) {
            archives.push(ChainArchive {
                path,
                entry_name: file_name.clone(),
                start,
                compressed: false,
            });
            continue;
        }

        if let Some((start, entry_name)) = parse_chain_acc_zip_start(&file_name) {
            archives.push(ChainArchive {
                path,
                entry_name,
                start,
                compressed: true,
            });
        }
    }
    Ok(archives)
}

fn parse_chain_acc_start(name: &str) -> Option<u32> {
    let name = name.strip_prefix("chain.")?;
    let name = name.strip_suffix(".acc")?;
    if name.is_empty() {
        return None;
    }
    name.parse().ok()
}

fn parse_chain_acc_zip_start(name: &str) -> Option<(u32, String)> {
    let base = name.strip_suffix(".zip")?;
    parse_chain_acc_start(base).map(|start| (start, base.to_string()))
}

struct ImportResponder {
    completion: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

#[async_trait]
impl Actor for ImportResponder {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if message.is::<ImportCompleted>() {
            if let Ok(mut guard) = self.completion.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(());
                }
            }
            let _ = ctx.stop_self();
        }
        Ok(())
    }
}
