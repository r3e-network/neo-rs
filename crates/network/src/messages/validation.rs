//! Neo N3 Message Validation
//!
//! This module provides comprehensive validation for all Neo N3 protocol messages,
//! ensuring strict compliance with the Neo N3 protocol specification.

use crate::{NetworkError, NetworkMessage, NetworkResult as Result, ProtocolMessage};
use neo_core::{Transaction, UInt160, UInt256};
use neo_ledger::{Block, BlockHeader};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, warn};

/// Maximum message size allowed (16MB)
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Maximum number of inventory items in a single message
pub const MAX_INVENTORY_ITEMS: usize = 1000;

/// Maximum number of headers in a single message
pub const MAX_HEADERS_PER_MESSAGE: usize = 2000;

/// Maximum number of addresses in addr message
pub const MAX_ADDRESSES_PER_MESSAGE: usize = 1000;

/// Maximum user agent length
pub const MAX_USER_AGENT_LENGTH: usize = 1024;

/// Minimum supported protocol version
pub const MIN_PROTOCOL_VERSION: u32 = 0;

/// Maximum supported protocol version
pub const MAX_PROTOCOL_VERSION: u32 = 1;

/// Future timestamp tolerance (15 seconds)
pub const FUTURE_TIMESTAMP_TOLERANCE: u64 = 15000;

/// Message validator that enforces Neo N3 protocol rules
pub struct MessageValidator {
    /// Network magic number for validation
    magic: u32,
    /// Maximum message size
    max_message_size: usize,
    /// Current blockchain height (for validation context)
    current_height: u32,
}

impl MessageValidator {
    /// Creates a new message validator
    pub fn new(magic: u32, current_height: u32) -> Self {
        Self {
            magic,
            max_message_size: MAX_MESSAGE_SIZE,
            current_height,
        }
    }

    /// Updates the current blockchain height for validation context
    pub fn update_height(&mut self, height: u32) {
        self.current_height = height;
    }

    /// Validates a complete network message
    pub fn validate_message(&self, message: &NetworkMessage) -> Result<()> {
        // 1. Validate message header
        self.validate_message_header(message)?;

        // 2. Validate payload size
        if message.serialized_size() > self.max_message_size {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Message size {} exceeds maximum {}",
                    message.serialized_size(),
                    self.max_message_size
                ),
            });
        }

        // 3. Validate magic number
        if message.header.magic != self.magic {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Invalid magic number: expected {:#x}, got {:#x}",
                    self.magic, message.header.magic
                ),
            });
        }

        // 4. Validate specific payload
        self.validate_payload(&message.payload)?;

        debug!("Message validation passed for {:?}", message.payload);
        Ok(())
    }

    /// Validates message header fields
    fn validate_message_header(&self, message: &NetworkMessage) -> Result<()> {
        // Check if command is recognized
        let command_str = message.header.command.to_string();
        if command_str.is_empty() || command_str.len() > 12 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("Invalid command: '{}'", command_str),
            });
        }

        // Validate checksum - it's always present in Neo N3
        let payload_bytes = self.serialize_payload(&message.payload)?;
        let calculated_checksum = Self::calculate_checksum(&payload_bytes);
        if message.header.checksum != calculated_checksum {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Checksum mismatch: expected {:#x}, calculated {:#x}",
                    calculated_checksum, message.header.checksum
                ),
            });
        }

        Ok(())
    }

    /// Helper method to serialize payload for checksum calculation
    fn serialize_payload(&self, payload: &ProtocolMessage) -> Result<Vec<u8>> {
        payload
            .to_bytes()
            .map_err(|e| NetworkError::MessageSerialization {
                message_type: "ProtocolMessage".to_string(),
                reason: e.to_string(),
            })
    }

    /// Calculates checksum for payload (SHA256(SHA256(payload))) - matches MessageHeader implementation
    fn calculate_checksum(payload: &[u8]) -> u32 {
        use sha2::{Digest, Sha256};
        let first_hash = Sha256::digest(payload);
        let second_hash = Sha256::digest(&first_hash);
        u32::from_le_bytes([
            second_hash[0],
            second_hash[1],
            second_hash[2],
            second_hash[3],
        ])
    }

    /// Helper method to check if an IP address is private
    fn is_private_ip(ip: &std::net::IpAddr) -> bool {
        match ip {
            std::net::IpAddr::V4(ipv4) => {
                // RFC 1918 private address ranges
                ipv4.octets()[0] == 10
                    || (ipv4.octets()[0] == 172 && ipv4.octets()[1] >= 16 && ipv4.octets()[1] <= 31)
                    || (ipv4.octets()[0] == 192 && ipv4.octets()[1] == 168)
            }
            std::net::IpAddr::V6(ipv6) => {
                // RFC 4193 private address range (fc00::/7)
                ipv6.segments()[0] & 0xfe00 == 0xfc00
            }
        }
    }

    /// Validates protocol message payload based on type
    fn validate_payload(&self, payload: &ProtocolMessage) -> Result<()> {
        match payload {
            ProtocolMessage::Version {
                version,
                services,
                timestamp,
                port,
                nonce,
                user_agent,
                start_height,
                relay,
            } => self.validate_version_message(
                *version,
                *services,
                *timestamp,
                *port,
                *nonce,
                user_agent,
                *start_height,
                *relay,
            ),

            ProtocolMessage::Verack => {
                // Verack has no payload to validate
                Ok(())
            }

            ProtocolMessage::GetAddr => {
                // GetAddr has no payload to validate
                Ok(())
            }

            ProtocolMessage::Addr { addresses } => self.validate_addr_message(addresses),

            ProtocolMessage::Ping { nonce } => self.validate_ping_message(*nonce),

            ProtocolMessage::Pong { nonce } => self.validate_pong_message(*nonce),

            ProtocolMessage::GetHeaders {
                hash_start,
                hash_stop,
            } => self.validate_get_headers_message(hash_start, hash_stop),

            ProtocolMessage::Headers { headers } => self.validate_headers_message(headers),

            ProtocolMessage::GetBlocks {
                hash_start,
                hash_stop,
            } => self.validate_get_blocks_message(hash_start, hash_stop),

            ProtocolMessage::GetBlockByIndex { index_start, count } => {
                self.validate_get_block_by_index_message(*index_start, *count)
            }

            ProtocolMessage::Tx { transaction } => self.validate_transaction_message(transaction),

            ProtocolMessage::Block { block } => self.validate_block_message(block),

            ProtocolMessage::Inv { inventory } => self.validate_inventory_message(inventory, "inv"),

            ProtocolMessage::GetData { inventory } => {
                self.validate_inventory_message(inventory, "getdata")
            }

            ProtocolMessage::NotFound { inventory } => {
                self.validate_inventory_message(inventory, "notfound")
            }

            ProtocolMessage::Mempool => {
                // Mempool has no payload to validate
                Ok(())
            }

            _ => {
                warn!("Unknown message type received");
                Ok(()) // Allow unknown messages for forward compatibility
            }
        }
    }

    /// Validates version message (matches C# Neo protocol validation exactly)
    fn validate_version_message(
        &self,
        version: u32,
        services: u64,
        timestamp: u64,
        port: u16,
        nonce: u32,
        user_agent: &str,
        start_height: u32,
        relay: bool,
    ) -> Result<()> {
        // 1. Validate protocol version
        if version < MIN_PROTOCOL_VERSION || version > MAX_PROTOCOL_VERSION {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("Unsupported protocol version: {}", version),
            });
        }

        // 2. Validate services field (should be valid bitmask)
        // Services field is a bitmask, all values are technically valid

        // 3. Validate timestamp (not too far in future)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if timestamp > current_time + FUTURE_TIMESTAMP_TOLERANCE {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Version timestamp too far in future: {} vs {}",
                    timestamp, current_time
                ),
            });
        }

        // 4. Validate port (non-zero for valid peers)
        if port == 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Version message has invalid port 0".to_string(),
            });
        }

        // 5. Validate user agent length
        if user_agent.len() > MAX_USER_AGENT_LENGTH {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "User agent too long: {} characters (max {})",
                    user_agent.len(),
                    MAX_USER_AGENT_LENGTH
                ),
            });
        }

        // 6. Validate user agent contains only printable ASCII
        if !user_agent.chars().all(|c| c.is_ascii() && !c.is_control()) {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "User agent contains invalid characters".to_string(),
            });
        }

        // 7. Validate start height is reasonable
        if start_height > self.current_height + 1000000 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Start height {} seems unreasonably high (current: {})",
                    start_height, self.current_height
                ),
            });
        }

        // 8. Validate nonce is non-zero
        if nonce == 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Version nonce cannot be zero".to_string(),
            });
        }

        debug!(
            "Version message validation passed: version={}, start_height={}, user_agent='{}'",
            version, start_height, user_agent
        );
        Ok(())
    }

    /// Validates addr message
    fn validate_addr_message(&self, addresses: &[SocketAddr]) -> Result<()> {
        if addresses.len() > MAX_ADDRESSES_PER_MESSAGE {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Too many addresses in addr message: {} (max {})",
                    addresses.len(),
                    MAX_ADDRESSES_PER_MESSAGE
                ),
            });
        }

        // Validate each address
        for addr in addresses {
            // Check for valid port
            if addr.port() == 0 {
                return Err(NetworkError::InvalidMessage {
                    peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                    message_type: "unknown".to_string(),
                    reason: format!("Invalid address with port 0: {}", addr),
                });
            }

            // Check for localhost/private addresses in mainnet context
            if self.magic == 0x334f454e {
                // MainNet magic
                if addr.ip().is_loopback() || Self::is_private_ip(&addr.ip()) {
                    return Err(NetworkError::InvalidMessage {
                        peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                        message_type: "unknown".to_string(),
                        reason: format!(
                            "Private/loopback address not allowed in mainnet: {}",
                            addr
                        ),
                    });
                }
            }
        }

        Ok(())
    }

    /// Validates ping message
    fn validate_ping_message(&self, nonce: u32) -> Result<()> {
        if nonce == 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Ping nonce cannot be zero".to_string(),
            });
        }
        Ok(())
    }

    /// Validates pong message
    fn validate_pong_message(&self, nonce: u32) -> Result<()> {
        if nonce == 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Pong nonce cannot be zero".to_string(),
            });
        }
        Ok(())
    }

    /// Validates get headers message
    fn validate_get_headers_message(
        &self,
        hash_start: &[UInt256],
        hash_stop: &UInt256,
    ) -> Result<()> {
        if hash_start.is_empty() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "GetHeaders must have at least one start hash".to_string(),
            });
        }

        if hash_start.len() > 100 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Too many start hashes in GetHeaders: {} (max 100)",
                    hash_start.len()
                ),
            });
        }

        // Validate that hashes are not all zero (except for genesis requests)
        let all_zero = hash_start.iter().all(|h| h.is_zero());
        if all_zero && !hash_stop.is_zero() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Invalid GetHeaders: all start hashes are zero but stop hash is not"
                    .to_string(),
            });
        }

        Ok(())
    }

    /// Validates headers message
    fn validate_headers_message(&self, headers: &[BlockHeader]) -> Result<()> {
        if headers.len() > MAX_HEADERS_PER_MESSAGE {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Too many headers: {} (max {})",
                    headers.len(),
                    MAX_HEADERS_PER_MESSAGE
                ),
            });
        }

        // Validate each header
        for (i, header) in headers.iter().enumerate() {
            self.validate_block_header(header)
                .map_err(|e| NetworkError::InvalidMessage {
                    peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                    message_type: "unknown".to_string(),
                    reason: format!("Invalid header at index {}: {}", i, e),
                })?;
        }

        // Validate headers are in sequential order
        for i in 1..headers.len() {
            if headers[i].index != headers[i - 1].index + 1 {
                return Err(NetworkError::InvalidMessage {
                    peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                    message_type: "unknown".to_string(),
                    reason: format!(
                        "Headers not in sequential order: {} followed by {}",
                        headers[i - 1].index,
                        headers[i].index
                    ),
                });
            }

            if headers[i].previous_hash != headers[i - 1].hash() {
                return Err(NetworkError::InvalidMessage {
                    peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                    message_type: "unknown".to_string(),
                    reason: format!(
                        "Header chain broken at index {}: previous hash mismatch",
                        headers[i].index
                    ),
                });
            }
        }

        Ok(())
    }

    /// Validates get blocks message
    fn validate_get_blocks_message(
        &self,
        hash_start: &[UInt256],
        hash_stop: &UInt256,
    ) -> Result<()> {
        // Same validation as get headers
        self.validate_get_headers_message(hash_start, hash_stop)
    }

    /// Validates get block by index message
    fn validate_get_block_by_index_message(&self, index_start: u32, count: u16) -> Result<()> {
        if count == 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "GetBlockByIndex count cannot be zero".to_string(),
            });
        }

        if count > 500 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("GetBlockByIndex count too large: {} (max 500)", count),
            });
        }

        // Check for reasonable index range
        if index_start > self.current_height + 1000000 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "GetBlockByIndex start index {} seems unreasonably high (current height: {})",
                    index_start, self.current_height
                ),
            });
        }

        Ok(())
    }

    /// Validates transaction message
    fn validate_transaction_message(&self, transaction: &Transaction) -> Result<()> {
        // 1. Validate transaction size
        let tx_size = transaction.size();
        if tx_size > 102400 {
            // 100KB max transaction size
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("Transaction size {} exceeds maximum 102400 bytes", tx_size),
            });
        }

        // 2. Validate transaction version
        if transaction.version() != 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("Invalid transaction version: {}", transaction.version()),
            });
        }

        // 3. Validate fees are non-negative
        if transaction.system_fee() < 0 || transaction.network_fee() < 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Transaction fees cannot be negative".to_string(),
            });
        }

        // 4. Validate script is not empty
        if transaction.script().is_empty() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Transaction script cannot be empty".to_string(),
            });
        }

        // 5. Validate valid until block
        if transaction.valid_until_block() <= self.current_height {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Transaction expired: valid until {}, current height {}",
                    transaction.valid_until_block(),
                    self.current_height
                ),
            });
        }

        // 6. Validate witnesses exist
        if transaction.witnesses().is_empty() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Transaction must have at least one witness".to_string(),
            });
        }

        debug!("Transaction message validation passed");
        Ok(())
    }

    /// Validates block message
    fn validate_block_message(&self, block: &Block) -> Result<()> {
        // 1. Validate block header
        self.validate_block_header(&block.header)?;

        // 2. Validate block has transactions
        if block.transactions.is_empty() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Block cannot have zero transactions".to_string(),
            });
        }

        // 3. Validate block size
        let block_size = block.size();
        if block_size > 1_048_576 {
            // 1MB max block size
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("Block size {} exceeds maximum 1MB", block_size),
            });
        }

        // 4. Validate each transaction
        for (i, tx) in block.transactions.iter().enumerate() {
            self.validate_transaction_message(tx)
                .map_err(|e| NetworkError::InvalidMessage {
                    peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                    message_type: "unknown".to_string(),
                    reason: format!("Invalid transaction {} in block: {}", i, e),
                })?;
        }

        // 5. Validate merkle root
        let calculated_merkle = block.calculate_merkle_root();
        if calculated_merkle != block.header.merkle_root {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Block merkle root mismatch".to_string(),
            });
        }

        debug!(
            "Block message validation passed for block {}",
            block.index()
        );
        Ok(())
    }

    /// Validates inventory message (inv, getdata, notfound)
    fn validate_inventory_message(
        &self,
        inventory: &[crate::InventoryItem],
        msg_type: &str,
    ) -> Result<()> {
        if inventory.len() > MAX_INVENTORY_ITEMS {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Too many inventory items in {}: {} (max {})",
                    msg_type,
                    inventory.len(),
                    MAX_INVENTORY_ITEMS
                ),
            });
        }

        // Validate each inventory item
        for (i, item) in inventory.iter().enumerate() {
            // Check for valid inventory type
            match item.item_type {
                crate::InventoryType::Transaction => {
                    // Hash should not be zero for transaction
                    if item.hash.is_zero() {
                        return Err(NetworkError::InvalidMessage {
                            peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                            message_type: "unknown".to_string(),
                            reason: format!(
                                "Invalid zero hash for transaction inventory item at index {}",
                                i
                            ),
                        });
                    }
                }
                crate::InventoryType::Block => {
                    // Hash should not be zero for block
                    if item.hash.is_zero() {
                        return Err(NetworkError::InvalidMessage {
                            peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                            message_type: "unknown".to_string(),
                            reason: format!(
                                "Invalid zero hash for block inventory item at index {}",
                                i
                            ),
                        });
                    }
                }
                crate::InventoryType::Consensus => {
                    // Hash should not be zero for consensus
                    if item.hash.is_zero() {
                        return Err(NetworkError::InvalidMessage {
                            peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                            message_type: "unknown".to_string(),
                            reason: format!(
                                "Invalid zero hash for consensus inventory item at index {}",
                                i
                            ),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Validates a block header (comprehensive validation matching C# Neo)
    fn validate_block_header(&self, header: &BlockHeader) -> Result<()> {
        // 1. Validate timestamp
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if header.timestamp > current_time + FUTURE_TIMESTAMP_TOLERANCE {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!(
                    "Block timestamp too far in future: {} vs {}",
                    header.timestamp, current_time
                ),
            });
        }

        // 2. Validate merkle root is not zero
        if header.merkle_root.is_zero() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Block merkle root cannot be zero".to_string(),
            });
        }

        // 3. Validate next consensus is not zero
        if header.next_consensus.is_zero() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Block next consensus cannot be zero".to_string(),
            });
        }

        // 4. Validate witnesses exist
        if header.witnesses.is_empty() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Block header must have at least one witness".to_string(),
            });
        }

        // 5. Validate version
        if header.version != 0 {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: format!("Invalid block version: {}", header.version),
            });
        }

        // 6. For non-genesis blocks, validate previous hash
        if header.index > 0 && header.previous_hash.is_zero() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Non-genesis block cannot have zero previous hash".to_string(),
            });
        }

        // 7. For genesis block, validate previous hash is zero
        if header.index == 0 && !header.previous_hash.is_zero() {
            return Err(NetworkError::InvalidMessage {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                message_type: "unknown".to_string(),
                reason: "Genesis block must have zero previous hash".to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageCommand, NetworkMessage};
    use neo_core::{BlockHeader, Transaction, UInt256, Witness};
    use neo_ledger::Block;

    /// Helper function to create test validator
    fn create_test_validator() -> MessageValidator {
        MessageValidator::new(0x3554334e, 1000)
    }

    /// Helper function to create valid test block header
    fn create_test_block_header(index: u32) -> BlockHeader {
        BlockHeader {
            version: 0,
            previous_hash: if index == 0 {
                UInt256::zero()
            } else {
                UInt256::new([1; 32])
            },
            merkle_root: UInt256::new([2; 32]),
            timestamp: 1640995200000, // Valid timestamp
            nonce: 1234567890,
            index,
            primary_index: 0,
            next_consensus: UInt160::new([3; 20]),
            witnesses: vec![Witness::default()],
        }
    }

    /// Helper function to create valid test block
    fn create_test_block(index: u32) -> Block {
        Block {
            header: create_test_block_header(index),
            transactions: vec![],
        }
    }

    #[test]
    fn test_message_validator_creation() {
        let network_magic = 0x3554334e;
        let max_block_height = 1000;
        let validator = MessageValidator::new(network_magic, max_block_height);

        assert_eq!(validator.network_magic, network_magic);
        assert_eq!(validator.max_block_height, max_block_height);
    }

    #[test]
    fn test_version_message_validation() {
        let validator = create_test_validator();

        // Valid version message
        let version_msg = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&version_msg).is_ok());

        // Invalid version - too high
        let invalid_version = ProtocolMessage::Version {
            version: 999,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&invalid_version).is_err());

        // Invalid services
        let invalid_services = ProtocolMessage::Version {
            version: 0,
            services: 0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&invalid_services).is_err());

        // Invalid port
        let invalid_port = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 0,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&invalid_port).is_err());

        // Invalid nonce
        let invalid_nonce = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 0,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&invalid_nonce).is_err());

        // Invalid user agent - too long
        let long_user_agent = "a".repeat(256);
        let invalid_user_agent = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: long_user_agent,
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&invalid_user_agent).is_err());

        // Invalid start height - too high
        let invalid_start_height = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 1001, // Higher than max_block_height
            relay: true,
        };

        assert!(validator.validate_payload(&invalid_start_height).is_err());
    }

    #[test]
    fn test_ping_pong_message_validation() {
        let validator = create_test_validator();

        // Valid ping
        let ping_msg = ProtocolMessage::Ping { nonce: 12345 };
        assert!(validator.validate_payload(&ping_msg).is_ok());

        // Valid pong
        let pong_msg = ProtocolMessage::Pong { nonce: 12345 };
        assert!(validator.validate_payload(&pong_msg).is_ok());

        // Invalid ping - zero nonce
        let invalid_ping = ProtocolMessage::Ping { nonce: 0 };
        assert!(validator.validate_payload(&invalid_ping).is_err());

        // Invalid pong - zero nonce
        let invalid_pong = ProtocolMessage::Pong { nonce: 0 };
        assert!(validator.validate_payload(&invalid_pong).is_err());
    }

    #[test]
    fn test_addr_message_validation() {
        let validator = create_test_validator();

        // Valid addr message
        let addr_msg = ProtocolMessage::Addr {
            addresses: vec!["1.2.3.4:20333".parse().unwrap()],
        };
        assert!(validator.validate_payload(&addr_msg).is_ok());

        // Valid addr message with multiple addresses
        let multi_addr_msg = ProtocolMessage::Addr {
            addresses: vec![
                "1.2.3.4:20333".parse().unwrap(),
                "5.6.7.8:20333".parse().unwrap(),
                "9.10.11.12:20333".parse().unwrap(),
            ],
        };
        assert!(validator.validate_payload(&multi_addr_msg).is_ok());

        // Invalid addr - empty addresses
        let empty_addr = ProtocolMessage::Addr { addresses: vec![] };
        assert!(validator.validate_payload(&empty_addr).is_err());

        // Invalid addr - too many addresses
        let mut many_addresses = Vec::new();
        for i in 0..1001 {
            many_addresses.push(format!("192.168.1.{}:20333", i % 255).parse().unwrap());
        }
        let invalid_addr = ProtocolMessage::Addr {
            addresses: many_addresses,
        };
        assert!(validator.validate_payload(&invalid_addr).is_err());
    }

    #[test]
    fn test_verack_getaddr_validation() {
        let validator = create_test_validator();

        // Verack and GetAddr have no payload, so they should always be valid
        assert!(validator.validate_payload(&ProtocolMessage::Verack).is_ok());
        assert!(
            validator
                .validate_payload(&ProtocolMessage::GetAddr)
                .is_ok()
        );
    }

    #[test]
    fn test_get_headers_message_validation() {
        let validator = create_test_validator();

        // Valid get headers message
        let hash_start = vec![UInt256::new([1; 32])];
        let hash_stop = UInt256::new([2; 32]);
        let get_headers_msg = ProtocolMessage::GetHeaders {
            hash_start,
            hash_stop,
        };
        assert!(validator.validate_payload(&get_headers_msg).is_ok());

        // Invalid - empty hash_start
        let invalid_get_headers = ProtocolMessage::GetHeaders {
            hash_start: vec![],
            hash_stop: UInt256::new([2; 32]),
        };
        assert!(validator.validate_payload(&invalid_get_headers).is_err());

        // Invalid - too many start hashes
        let many_hashes: Vec<UInt256> = (0..100).map(|_| UInt256::new([1; 32])).collect();
        let invalid_many_hashes = ProtocolMessage::GetHeaders {
            hash_start: many_hashes,
            hash_stop: UInt256::new([2; 32]),
        };
        assert!(validator.validate_payload(&invalid_many_hashes).is_err());

        // Invalid - zero hash_stop
        let invalid_hash_stop = ProtocolMessage::GetHeaders {
            hash_start: vec![UInt256::new([1; 32])],
            hash_stop: UInt256::zero(),
        };
        assert!(validator.validate_payload(&invalid_hash_stop).is_err());
    }

    #[test]
    fn test_headers_message_validation() {
        let validator = create_test_validator();

        // Valid headers message
        let headers = vec![create_test_block_header(1), create_test_block_header(2)];
        let headers_msg = ProtocolMessage::Headers { headers };
        assert!(validator.validate_payload(&headers_msg).is_ok());

        // Valid empty headers
        let empty_headers_msg = ProtocolMessage::Headers { headers: vec![] };
        assert!(validator.validate_payload(&empty_headers_msg).is_ok());

        // Invalid - too many headers
        let many_headers: Vec<BlockHeader> = (0..2001).map(create_test_block_header).collect();
        let invalid_headers_msg = ProtocolMessage::Headers {
            headers: many_headers,
        };
        assert!(validator.validate_payload(&invalid_headers_msg).is_err());

        // Invalid - header with invalid index
        let mut invalid_header = create_test_block_header(1001); // Higher than max_block_height
        let invalid_headers_msg = ProtocolMessage::Headers {
            headers: vec![invalid_header],
        };
        assert!(validator.validate_payload(&invalid_headers_msg).is_err());
    }

    #[test]
    fn test_get_blocks_message_validation() {
        let validator = create_test_validator();

        // Valid get blocks message
        let hash_start = vec![UInt256::new([1; 32])];
        let hash_stop = UInt256::new([2; 32]);
        let get_blocks_msg = ProtocolMessage::GetBlocks {
            hash_start,
            hash_stop,
        };
        assert!(validator.validate_payload(&get_blocks_msg).is_ok());

        // Invalid - empty hash_start
        let invalid_get_blocks = ProtocolMessage::GetBlocks {
            hash_start: vec![],
            hash_stop: UInt256::new([2; 32]),
        };
        assert!(validator.validate_payload(&invalid_get_blocks).is_err());
    }

    #[test]
    fn test_get_block_by_index_message_validation() {
        let validator = create_test_validator();

        // Valid get block by index message
        let get_block_msg = ProtocolMessage::GetBlockByIndex {
            index_start: 0,
            count: 500,
        };
        assert!(validator.validate_payload(&get_block_msg).is_ok());

        // Invalid - count too high
        let invalid_count = ProtocolMessage::GetBlockByIndex {
            index_start: 0,
            count: 501,
        };
        assert!(validator.validate_payload(&invalid_count).is_err());

        // Invalid - index_start too high
        let invalid_index = ProtocolMessage::GetBlockByIndex {
            index_start: 1001,
            count: 1,
        };
        assert!(validator.validate_payload(&invalid_index).is_err());

        // Valid - zero count (allowed)
        let zero_count = ProtocolMessage::GetBlockByIndex {
            index_start: 0,
            count: 0,
        };
        assert!(validator.validate_payload(&zero_count).is_ok());
    }

    #[test]
    fn test_block_message_validation() {
        let validator = create_test_validator();

        // Valid block message
        let block = create_test_block(1);
        let block_msg = ProtocolMessage::Block { block };
        assert!(validator.validate_payload(&block_msg).is_ok());

        // Valid genesis block
        let genesis_block = create_test_block(0);
        let genesis_msg = ProtocolMessage::Block {
            block: genesis_block,
        };
        assert!(validator.validate_payload(&genesis_msg).is_ok());

        // Invalid - block index too high
        let invalid_block = create_test_block(1001);
        let invalid_block_msg = ProtocolMessage::Block {
            block: invalid_block,
        };
        assert!(validator.validate_payload(&invalid_block_msg).is_err());
    }

    #[test]
    fn test_block_header_validation_detailed() {
        let validator = create_test_validator();

        // Valid block header
        let valid_header = create_test_block_header(1);
        assert!(validator.validate_block_header(&valid_header).is_ok());

        // Invalid - zero merkle root
        let mut invalid_merkle = create_test_block_header(1);
        invalid_merkle.merkle_root = UInt256::zero();
        assert!(validator.validate_block_header(&invalid_merkle).is_err());

        // Invalid - zero next consensus
        let mut invalid_consensus = create_test_block_header(1);
        invalid_consensus.next_consensus = UInt160::zero();
        assert!(validator.validate_block_header(&invalid_consensus).is_err());

        // Invalid - no witnesses
        let mut invalid_witnesses = create_test_block_header(1);
        invalid_witnesses.witnesses = vec![];
        assert!(validator.validate_block_header(&invalid_witnesses).is_err());

        // Invalid - wrong version
        let mut invalid_version = create_test_block_header(1);
        invalid_version.version = 1;
        assert!(validator.validate_block_header(&invalid_version).is_err());

        // Invalid - non-genesis with zero previous hash
        let mut invalid_previous = create_test_block_header(1);
        invalid_previous.previous_hash = UInt256::zero();
        assert!(validator.validate_block_header(&invalid_previous).is_err());

        // Invalid - genesis with non-zero previous hash
        let mut invalid_genesis = create_test_block_header(0);
        invalid_genesis.previous_hash = UInt256::new([1; 32]);
        assert!(validator.validate_block_header(&invalid_genesis).is_err());

        // Valid genesis block
        let mut valid_genesis = create_test_block_header(0);
        valid_genesis.previous_hash = UInt256::zero();
        assert!(validator.validate_block_header(&valid_genesis).is_ok());
    }

    #[test]
    fn test_transaction_message_validation() {
        let validator = create_test_validator();

        // Valid transaction message
        let transaction = Transaction::new();
        let tx_msg = ProtocolMessage::Transaction { transaction };
        assert!(validator.validate_payload(&tx_msg).is_ok());
    }

    #[test]
    fn test_inventory_message_validation() {
        let validator = create_test_validator();

        // Valid inventory message
        let hashes = vec![UInt256::new([1; 32]), UInt256::new([2; 32])];
        let inv_msg = ProtocolMessage::Inventory {
            inv_type: 0x2c, // Block type
            hashes,
        };
        assert!(validator.validate_payload(&inv_msg).is_ok());

        // Invalid - too many hashes
        let many_hashes: Vec<UInt256> = (0..50001)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[0] = (i % 256) as u8;
                UInt256::new(bytes)
            })
            .collect();
        let invalid_inv = ProtocolMessage::Inventory {
            inv_type: 0x2c,
            hashes: many_hashes,
        };
        assert!(validator.validate_payload(&invalid_inv).is_err());

        // Invalid - empty hashes
        let empty_inv = ProtocolMessage::Inventory {
            inv_type: 0x2c,
            hashes: vec![],
        };
        assert!(validator.validate_payload(&empty_inv).is_err());
    }

    #[test]
    fn test_get_data_message_validation() {
        let validator = create_test_validator();

        // Valid get data message
        let hashes = vec![UInt256::new([1; 32])];
        let get_data_msg = ProtocolMessage::GetData {
            inv_type: 0x2c,
            hashes,
        };
        assert!(validator.validate_payload(&get_data_msg).is_ok());

        // Invalid - too many hashes
        let many_hashes: Vec<UInt256> = (0..50001).map(|_| UInt256::new([1; 32])).collect();
        let invalid_get_data = ProtocolMessage::GetData {
            inv_type: 0x2c,
            hashes: many_hashes,
        };
        assert!(validator.validate_payload(&invalid_get_data).is_err());
    }

    #[test]
    fn test_network_message_validation() {
        let validator = create_test_validator();

        // Valid network message
        let version_payload = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        let network_msg = NetworkMessage {
            magic: 0x3554334e,
            command: MessageCommand::Version,
            payload: version_payload,
            checksum: 0, // Would be calculated properly in real implementation
        };

        assert!(validator.validate_message(&network_msg).is_ok());

        // Invalid - wrong magic
        let mut invalid_magic = network_msg.clone();
        invalid_magic.magic = 0x12345678;
        assert!(validator.validate_message(&invalid_magic).is_err());
    }

    #[test]
    fn test_message_size_limits() {
        let validator = create_test_validator();

        // Test that very large messages are rejected
        // This would be handled at the network layer, but validation should also catch it

        // Large user agent in version message
        let large_version = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            port: 20333,
            nonce: 12345,
            user_agent: "a".repeat(1000), // Very large user agent
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&large_version).is_err());
    }

    #[test]
    fn test_edge_case_validation() {
        let validator = create_test_validator();

        // Test edge cases for numeric values

        // Version message with maximum valid values
        let max_values_version = ProtocolMessage::Version {
            version: 0,
            services: u64::MAX,
            timestamp: u64::MAX,
            port: 65535, // Max port
            nonce: u64::MAX,
            user_agent: "valid".to_string(),
            start_height: 1000, // Max allowed by validator
            relay: true,
        };

        // Should still validate correctly
        assert!(validator.validate_payload(&max_values_version).is_ok());
    }

    #[test]
    fn test_timestamp_validation() {
        let validator = create_test_validator();

        // Test timestamp validation in version messages
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Valid current timestamp
        let valid_time_version = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: current_time,
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&valid_time_version).is_ok());

        // Very old timestamp (should still be valid as it's just a timestamp)
        let old_time_version = ProtocolMessage::Version {
            version: 0,
            services: 1,
            timestamp: 1000000, // Very old timestamp
            port: 20333,
            nonce: 12345,
            user_agent: "neo-rs/1.0".to_string(),
            start_height: 500,
            relay: true,
        };

        assert!(validator.validate_payload(&old_time_version).is_ok());
    }
}
