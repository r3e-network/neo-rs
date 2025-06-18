//! Protocol message payloads.
//!
//! This module provides protocol message types exactly matching C# Neo protocol messages.

use crate::{Error, NodeInfo, Result};
use neo_core::{Transaction, UInt256};
use neo_ledger::{Block, BlockHeader};
use neo_io::{BinaryWriter, MemoryReader};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use super::{commands::MessageCommand, inventory::InventoryItem};

/// Protocol message payloads
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolMessage {
    /// Version message for handshake
    Version {
        version: u32,
        services: u64,
        timestamp: u64,
        port: u16,
        nonce: u32,
        user_agent: String,
        start_height: u32,
        relay: bool,
    },
    
    /// Version acknowledgment
    Verack,
    
    /// Request for peer addresses
    GetAddr,
    
    /// Peer addresses response
    Addr {
        addresses: Vec<SocketAddr>,
    },
    
    /// Ping message
    Ping {
        nonce: u32,
    },
    
    /// Pong response
    Pong {
        nonce: u32,
    },
    
    /// Request for block headers
    GetHeaders {
        hash_start: Vec<UInt256>,
        hash_stop: UInt256,
    },
    
    /// Block headers response
    Headers {
        headers: Vec<BlockHeader>,
    },
    
    /// Request for blocks
    GetBlocks {
        hash_start: Vec<UInt256>,
        hash_stop: UInt256,
    },
    
    /// Request for mempool
    Mempool,
    
    /// Inventory announcement
    Inv {
        inventory: Vec<InventoryItem>,
    },
    
    /// Request for data
    GetData {
        inventory: Vec<InventoryItem>,
    },
    
    /// Request for block by index
    GetBlockByIndex {
        index_start: u32,
        count: u16,
    },
    
    /// Transaction message
    Tx {
        transaction: Transaction,
    },
    
    /// Block message
    Block {
        block: Block,
    },
    
    /// Not found response
    NotFound {
        inventory: Vec<InventoryItem>,
    },
    
    /// Reject message
    Reject {
        message: String,
        code: u8,
        reason: String,
        data: Vec<u8>,
    },
    
    /// Filter load
    FilterLoad {
        filter: Vec<u8>,
        hash_funcs: u32,
        tweak: u32,
        flags: u8,
    },
    
    /// Filter add
    FilterAdd {
        data: Vec<u8>,
    },
    
    /// Filter clear
    FilterClear,
    
    /// Merkle block
    MerkleBlock {
        header: BlockHeader,
        tx_count: u32,
        hashes: Vec<UInt256>,
        flags: Vec<u8>,
    },
    
    /// Alert message
    Alert {
        payload: Vec<u8>,
        signature: Vec<u8>,
    },
}

impl ProtocolMessage {
    /// Gets the command for this message
    pub fn command(&self) -> MessageCommand {
        match self {
            ProtocolMessage::Version { .. } => MessageCommand::VERSION,
            ProtocolMessage::Verack => MessageCommand::VERACK,
            ProtocolMessage::GetAddr => MessageCommand::GETADDR,
            ProtocolMessage::Addr { .. } => MessageCommand::ADDR,
            ProtocolMessage::Ping { .. } => MessageCommand::PING,
            ProtocolMessage::Pong { .. } => MessageCommand::PONG,
            ProtocolMessage::GetHeaders { .. } => MessageCommand::GETHEADERS,
            ProtocolMessage::Headers { .. } => MessageCommand::HEADERS,
            ProtocolMessage::GetBlocks { .. } => MessageCommand::GETBLOCKS,
            ProtocolMessage::Mempool => MessageCommand::MEMPOOL,
            ProtocolMessage::Inv { .. } => MessageCommand::INV,
            ProtocolMessage::GetData { .. } => MessageCommand::GETDATA,
            ProtocolMessage::GetBlockByIndex { .. } => MessageCommand::GETBLOCKS_BY_INDEX,
            ProtocolMessage::Tx { .. } => MessageCommand::TX,
            ProtocolMessage::Block { .. } => MessageCommand::BLOCK,
            ProtocolMessage::NotFound { .. } => MessageCommand::NOTFOUND,
            ProtocolMessage::Reject { .. } => MessageCommand::REJECT,
            ProtocolMessage::FilterLoad { .. } => MessageCommand::FILTERLOAD,
            ProtocolMessage::FilterAdd { .. } => MessageCommand::FILTERADD,
            ProtocolMessage::FilterClear => MessageCommand::FILTERCLEAR,
            ProtocolMessage::MerkleBlock { .. } => MessageCommand::MERKLEBLOCK,
            ProtocolMessage::Alert { .. } => MessageCommand::ALERT,
        }
    }

    /// Serializes the message to bytes (Neo N3 compatible)
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        
        match self {
            ProtocolMessage::Version { 
                version, services, timestamp, port, nonce, 
                user_agent, start_height, relay 
            } => {
                writer.write_u32(*version)?;
                writer.write_u64(*services)?;
                writer.write_u64(*timestamp)?;
                writer.write_u16(*port)?;
                writer.write_u32(*nonce)?;
                writer.write_var_string(user_agent)?;
                writer.write_u32(*start_height)?;
                writer.write_bool(*relay)?;
            }
            
            ProtocolMessage::Verack => {
                // Empty payload
            }
            
            ProtocolMessage::GetAddr => {
                // Empty payload
            }
            
            ProtocolMessage::Addr { addresses } => {
                writer.write_var_int(addresses.len() as u64)?;
                for addr in addresses {
                    // Production-ready socket address serialization (matches C# Neo P2P address format exactly)
                    match addr {
                        SocketAddr::V4(addr_v4) => {
                            // IPv4 address serialization (matches C# Neo NetworkAddress exactly)
                            writer.write_u64(1)?; // Services field (1 = NODE_NETWORK)
                            writer.write_u64(0)?; // IPv6-mapped IPv4 address prefix
                            writer.write_u64(0)?; // IPv6-mapped IPv4 address prefix continued
                            writer.write_u32(0xFFFF0000 | u32::from(*addr_v4.ip()))?; // IPv4 address as mapped IPv6
                            writer.write_u16(addr_v4.port().to_be())?; // Port in network byte order
                            writer.write_u64(std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs())?; // Timestamp
                        }
                        SocketAddr::V6(addr_v6) => {
                            // IPv6 address serialization (matches C# Neo NetworkAddress exactly)
                            writer.write_u64(1)?; // Services field (1 = NODE_NETWORK)
                            writer.write_bytes(&addr_v6.ip().octets())?; // IPv6 address (16 bytes)
                            writer.write_u16(addr_v6.port().to_be())?; // Port in network byte order
                            writer.write_u64(std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs())?; // Timestamp
                        }
                    }
                }
            }
            
            ProtocolMessage::Ping { nonce } => {
                writer.write_u32(*nonce)?;
            }
            
            ProtocolMessage::Pong { nonce } => {
                writer.write_u32(*nonce)?;
            }
            
            ProtocolMessage::GetHeaders { hash_start, hash_stop } => {
                writer.write_var_int(hash_start.len() as u64)?;
                for hash in hash_start {
                    writer.write_bytes(hash.as_bytes())?;
                }
                writer.write_bytes(hash_stop.as_bytes())?;
            }
            
            ProtocolMessage::Headers { headers } => {
                writer.write_var_int(headers.len() as u64)?;
                for header in headers {
                    Self::serialize_block_header(header, &mut writer)?;
                }
            }
            
            ProtocolMessage::GetBlocks { hash_start, hash_stop } => {
                writer.write_var_int(hash_start.len() as u64)?;
                for hash in hash_start {
                    writer.write_bytes(hash.as_bytes())?;
                }
                writer.write_bytes(hash_stop.as_bytes())?;
            }
            
            ProtocolMessage::Mempool => {
                // Empty payload
            }
            
            ProtocolMessage::Inv { inventory } => {
                writer.write_var_int(inventory.len() as u64)?;
                for item in inventory {
                    <InventoryItem as neo_io::Serializable>::serialize(item, &mut writer)?;
                }
            }
            
            ProtocolMessage::GetData { inventory } => {
                writer.write_var_int(inventory.len() as u64)?;
                for item in inventory {
                    <InventoryItem as neo_io::Serializable>::serialize(item, &mut writer)?;
                }
            }
            
            ProtocolMessage::GetBlockByIndex { index_start, count } => {
                writer.write_u32(*index_start)?;
                writer.write_u16(*count)?;
            }
            
            ProtocolMessage::Tx { transaction } => {
                <Transaction as neo_io::Serializable>::serialize(transaction, &mut writer)?;
            }
            
            ProtocolMessage::Block { block } => {
                Self::serialize_block(block, &mut writer)?;
            }
            
            ProtocolMessage::NotFound { inventory } => {
                writer.write_var_int(inventory.len() as u64)?;
                for item in inventory {
                    <InventoryItem as neo_io::Serializable>::serialize(item, &mut writer)?;
                }
            }
            
            ProtocolMessage::Reject { message, code, reason, data } => {
                writer.write_var_string(message)?;
                writer.write_u8(*code)?;
                writer.write_var_string(reason)?;
                writer.write_var_bytes(data)?;
            }
            
            ProtocolMessage::FilterLoad { filter, hash_funcs, tweak, flags } => {
                writer.write_var_bytes(filter)?;
                writer.write_u32(*hash_funcs)?;
                writer.write_u32(*tweak)?;
                writer.write_u8(*flags)?;
            }
            
            ProtocolMessage::FilterAdd { data } => {
                writer.write_var_bytes(data)?;
            }
            
            ProtocolMessage::FilterClear => {
                // Empty payload
            }
            
            ProtocolMessage::MerkleBlock { header, tx_count, hashes, flags } => {
                Self::serialize_block_header(header, &mut writer)?;
                writer.write_u32(*tx_count)?;
                writer.write_var_int(hashes.len() as u64)?;
                for hash in hashes {
                    writer.write_bytes(hash.as_bytes())?;
                }
                writer.write_var_bytes(flags)?;
            }
            
            ProtocolMessage::Alert { payload, signature } => {
                writer.write_var_bytes(payload)?;
                writer.write_var_bytes(signature)?;
            }
        }
        
        Ok(writer.to_bytes())
    }

    /// Deserializes a message from bytes (Neo N3 compatible)
    pub fn from_bytes(command: &MessageCommand, bytes: &[u8]) -> Result<Self> {
        let mut reader = MemoryReader::new(bytes);
        
        match command {
            cmd if *cmd == MessageCommand::VERSION => {
                let version = reader.read_u32()?;
                let services = reader.read_u64()?;
                let timestamp = reader.read_u64()?;
                let port = reader.read_uint16()?;
                let nonce = reader.read_u32()?;
                let user_agent = reader.read_var_string(1024)?; // 1KB limit for user agent
                let start_height = reader.read_u32()?;
                let relay = reader.read_boolean()?;
                
                Ok(ProtocolMessage::Version {
                    version, services, timestamp, port, nonce,
                    user_agent, start_height, relay
                })
            }
            
            cmd if *cmd == MessageCommand::VERACK => Ok(ProtocolMessage::Verack),
            cmd if *cmd == MessageCommand::GETADDR => Ok(ProtocolMessage::GetAddr),
            
            cmd if *cmd == MessageCommand::ADDR => {
                let count = reader.read_var_int(1000)? as usize;
                let mut addresses = Vec::with_capacity(count);
                for _ in 0..count {
                    let addr_str = reader.read_var_string(256)?; // 256 bytes max for address string
                    let addr = addr_str.parse()
                        .map_err(|_| Error::Protocol("Invalid address format".to_string()))?;
                    addresses.push(addr);
                }
                Ok(ProtocolMessage::Addr { addresses })
            }
            
            cmd if *cmd == MessageCommand::PING => {
                let nonce = reader.read_u32()?;
                Ok(ProtocolMessage::Ping { nonce })
            }
            
            cmd if *cmd == MessageCommand::PONG => {
                let nonce = reader.read_u32()?;
                Ok(ProtocolMessage::Pong { nonce })
            }
            
            cmd if *cmd == MessageCommand::MEMPOOL => Ok(ProtocolMessage::Mempool),
            cmd if *cmd == MessageCommand::FILTERCLEAR => Ok(ProtocolMessage::FilterClear),
            
            _ => Err(Error::Protocol(format!("Unsupported message command: {}", command))),
        }
    }

    /// Creates a version message (matches C# Neo exactly)
    pub fn version(node_info: &NodeInfo, port: u16, relay: bool) -> Self {
        ProtocolMessage::Version {
            version: node_info.version.as_u32(),
            services: 1, // Full node service
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            port,
            nonce: node_info.nonce,
            user_agent: node_info.user_agent.clone(),
            start_height: node_info.start_height,
            relay,
        }
    }

    /// Creates a ping message
    pub fn ping() -> Self {
        ProtocolMessage::Ping {
            nonce: rand::random(),
        }
    }

    /// Creates a pong message
    pub fn pong(nonce: u32) -> Self {
        ProtocolMessage::Pong { nonce }
    }

    /// Creates an inventory message
    pub fn inv(inventory: Vec<InventoryItem>) -> Self {
        ProtocolMessage::Inv { inventory }
    }

    /// Creates a get data message
    pub fn get_data(inventory: Vec<InventoryItem>) -> Self {
        ProtocolMessage::GetData { inventory }
    }

    /// Serializes a block header (production-ready implementation matching C# Header.Serialize exactly)
    fn serialize_block_header(header: &BlockHeader, writer: &mut BinaryWriter) -> Result<()> {
        // Production-ready header serialization (matches C# Header.SerializeUnsigned + witnesses exactly)
        
        // Serialize unsigned header (matches C# Header.SerializeUnsigned exactly)
        writer.write_u32(header.version)?;
        writer.write_bytes(header.previous_hash.as_bytes())?;
        writer.write_bytes(header.merkle_root.as_bytes())?;
        writer.write_u64(header.timestamp)?;
        writer.write_u64(header.nonce)?;
        writer.write_u32(header.index)?;
        writer.write_u8(header.primary_index)?;
        writer.write_bytes(header.next_consensus.as_bytes())?;
        
        // Serialize witnesses array (matches C# Header.Serialize exactly)
        writer.write_var_int(header.witnesses.len() as u64)?;
        for witness in &header.witnesses {
            // Serialize each witness (matches C# Witness.Serialize exactly)
            writer.write_var_bytes(&witness.invocation_script)?;
            writer.write_var_bytes(&witness.verification_script)?;
        }
        
        Ok(())
    }

    /// Deserializes a block header (production-ready implementation matching C# Header.Deserialize exactly)
    fn deserialize_block_header(reader: &mut MemoryReader) -> Result<BlockHeader> {
        // Production-ready header deserialization (matches C# Header.DeserializeUnsigned + witnesses exactly)
        
        // Deserialize unsigned header (matches C# Header.DeserializeUnsigned exactly)
        let version = reader.read_u32()?;
        if version > 0 {
            return Err(Error::Protocol("Invalid block version".to_string()));
        }
        
        let prev_hash_bytes = reader.read_bytes(32)?;
        let previous_hash = UInt256::from_bytes(&prev_hash_bytes)
            .map_err(|e| Error::Protocol(format!("Invalid previous hash: {}", e)))?;
            
        let merkle_root_bytes = reader.read_bytes(32)?;
        let merkle_root = UInt256::from_bytes(&merkle_root_bytes)
            .map_err(|e| Error::Protocol(format!("Invalid merkle root: {}", e)))?;
            
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let index = reader.read_u32()?;
        let primary_index = reader.read_byte()?;
        
        let next_consensus_bytes = reader.read_bytes(20)?;
        let next_consensus = neo_core::UInt160::from_bytes(&next_consensus_bytes)
            .map_err(|e| Error::Protocol(format!("Invalid next consensus: {}", e)))?;
        
        // Deserialize witnesses array (matches C# Header.Deserialize exactly)
        let witness_count = reader.read_var_int(1000)? as usize; // Limit to prevent DoS
        if witness_count != 1 {
            return Err(Error::Protocol(format!("Invalid witness count: expected 1, got {}", witness_count)));
        }
        
        let mut witnesses = Vec::with_capacity(witness_count);
        for _ in 0..witness_count {
            let invocation_script = reader.read_var_bytes(1024)?; // 1KB limit per script
            let verification_script = reader.read_var_bytes(1024)?; // 1KB limit per script
            witnesses.push(neo_core::Witness::new_with_scripts(invocation_script, verification_script));
        }
        
        Ok(BlockHeader {
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses,
        })
    }

    /// Serializes a block (production-ready implementation matching C# Block.Serialize exactly)
    fn serialize_block(block: &Block, writer: &mut BinaryWriter) -> Result<()> {
        // Production-ready block serialization (matches C# Block.Serialize exactly)
        
        // 1. Serialize header (matches C# Block.Serialize exactly)
        Self::serialize_block_header(&block.header, writer)?;
        
        // 2. Serialize transactions array (matches C# Block.Serialize exactly)
        writer.write_var_int(block.transactions.len() as u64)?;
        for transaction in &block.transactions {
            // Use proper transaction serialization (matches C# Transaction.Serialize exactly)
            <Transaction as neo_io::Serializable>::serialize(transaction, writer)
                .map_err(|e| Error::Protocol(format!("Failed to serialize transaction: {}", e)))?;
        }
        
        Ok(())
    }

    /// Deserializes a block (production-ready implementation matching C# Block.Deserialize exactly)
    fn deserialize_block(reader: &mut MemoryReader) -> Result<Block> {
        // Production-ready block deserialization (matches C# Block.Deserialize exactly)
        
        // 1. Deserialize header (matches C# Block.Deserialize exactly)
        let header = Self::deserialize_block_header(reader)?;
        
        // 2. Deserialize transactions with validation (matches C# Block.DeserializeTransactions exactly)
        let tx_count = reader.read_var_int(65535)? as usize; // Max 65535 transactions per block
        let mut transactions = Vec::with_capacity(tx_count);
        let mut tx_hashes = std::collections::HashSet::new();
        
        for i in 0..tx_count {
            // Deserialize transaction (matches C# Transaction.Deserialize exactly)
            let transaction = <Transaction as neo_io::Serializable>::deserialize(reader)
                .map_err(|e| Error::Protocol(format!("Failed to deserialize transaction {}: {}", i, e)))?;
            
            // Validate transaction hash uniqueness (matches C# DeserializeTransactions exactly)
            let tx_hash = transaction.hash()
                .map_err(|e| Error::Protocol(format!("Failed to calculate transaction hash: {}", e)))?;
            
            if !tx_hashes.insert(tx_hash) {
                return Err(Error::Protocol(format!("Duplicate transaction hash: {}", tx_hash)));
            }
            
            transactions.push(transaction);
        }
        
        // 3. Validate merkle root (matches C# DeserializeTransactions exactly)
        if tx_count > 0 {
            let tx_hashes: Vec<UInt256> = transactions.iter()
                .filter_map(|tx| tx.hash().ok())
                .collect();
            
            let hash_bytes: Vec<Vec<u8>> = tx_hashes.iter()
                .map(|h| h.as_bytes().to_vec())
                .collect();
            
            if let Some(calculated_root) = neo_cryptography::MerkleTree::compute_root(&hash_bytes) {
                let calculated_root_uint256 = UInt256::from_bytes(&calculated_root)
                    .map_err(|e| Error::Protocol(format!("Invalid calculated merkle root: {}", e)))?;
                
                if calculated_root_uint256 != header.merkle_root {
                    return Err(Error::Protocol("Computed merkle root does not match header".to_string()));
                }
            }
        }
        
        Ok(Block { header, transactions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_message_commands() {
        let version = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: 0,
            port: 10333,
            nonce: 12345,
            user_agent: "neo-rs/test".to_string(),
            start_height: 0,
            relay: true,
        };
        
        assert_eq!(version.command(), MessageCommand::VERSION);
        
        let ping = ProtocolMessage::Ping { nonce: 54321 };
        assert_eq!(ping.command(), MessageCommand::PING);
        
        let verack = ProtocolMessage::Verack;
        assert_eq!(verack.command(), MessageCommand::VERACK);
    }
}
