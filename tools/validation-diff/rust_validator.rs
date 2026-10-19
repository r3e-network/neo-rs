//! Rust 侧区块/交易验证规则包装器
//!
//! 简化版本：基于十六进制进行基本格式验证，不依赖完整反序列化

use neo_core::ProtocolSettings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockValidationTest {
    pub test_id: String,
    pub block_hex: String,
    pub expected_valid: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionValidationTest {
    pub test_id: String,
    pub tx_hex: String,
    pub expected_valid: bool,
    pub description: String,
}

pub struct RustValidator {
    _protocol_settings: ProtocolSettings,
}

impl RustValidator {
    pub fn new(protocol_settings: ProtocolSettings) -> Self {
        Self {
            _protocol_settings: protocol_settings,
        }
    }

    /// 验证区块（基于十六进制的基本验证）
    pub fn validate_block(&self, block_hex: &str) -> ValidationResult {
        // 1. 从十六进制解码区块
        let block_bytes = match hex::decode(block_hex) {
            Ok(bytes) => bytes,
            Err(e) => {
                return ValidationResult {
                    valid: false,
                    error_type: Some("DecodeError".to_string()),
                    error_message: Some(format!("Failed to decode block hex: {}", e)),
                }
            }
        };

        // 2. 基本格式验证
        // Block 最小长度：version(4) + prev_hash(32) + merkle_root(32) + timestamp(8) + nonce(8) + index(4) + primary(1) + next_consensus(20) + witness(至少2字节) = 至少 111 字节
        if block_bytes.len() < 111 {
            return ValidationResult {
                valid: false,
                error_type: Some("SizeError".to_string()),
                error_message: Some(format!(
                    "Block too small: {} bytes (minimum 111)",
                    block_bytes.len()
                )),
            };
        }

        // 3. 验证版本号（前 4 字节，小端序）
        let version = u32::from_le_bytes([
            block_bytes[0],
            block_bytes[1],
            block_bytes[2],
            block_bytes[3],
        ]);
        if version != 0 {
            return ValidationResult {
                valid: false,
                error_type: Some("VersionError".to_string()),
                error_message: Some(format!("Invalid version: {} (expected 0)", version)),
            };
        }

        // 4. 验证时间戳（字节 68-75，小端序）
        const MIN_TIMESTAMP: u64 = 1468595301000; // Neo 创世块时间
        let timestamp = u64::from_le_bytes([
            block_bytes[68],
            block_bytes[69],
            block_bytes[70],
            block_bytes[71],
            block_bytes[72],
            block_bytes[73],
            block_bytes[74],
            block_bytes[75],
        ]);

        if timestamp < MIN_TIMESTAMP {
            return ValidationResult {
                valid: false,
                error_type: Some("TimestampError".to_string()),
                error_message: Some(format!(
                    "Timestamp too early: {} (minimum {})",
                    timestamp, MIN_TIMESTAMP
                )),
            };
        }

        // 验证时间戳不能太远的未来
        const MAX_FUTURE_SECONDS: u64 = 15 * 60; // 15 分钟
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        if timestamp > now + MAX_FUTURE_SECONDS * 1000 {
            return ValidationResult {
                valid: false,
                error_type: Some("TimestampError".to_string()),
                error_message: Some(format!(
                    "Timestamp too far in future: {} (current: {})",
                    timestamp, now
                )),
            };
        }

        // 基本验证通过
        ValidationResult {
            valid: true,
            error_type: None,
            error_message: None,
        }
    }

    /// 验证交易（基于十六进制的基本验证）
    pub fn validate_transaction(&self, tx_hex: &str) -> ValidationResult {
        // 1. 从十六进制解码交易
        let tx_bytes = match hex::decode(tx_hex) {
            Ok(bytes) => bytes,
            Err(e) => {
                return ValidationResult {
                    valid: false,
                    error_type: Some("DecodeError".to_string()),
                    error_message: Some(format!("Failed to decode transaction hex: {}", e)),
                }
            }
        };

        // 2. 验证交易大小
        const MAX_TX_SIZE: usize = 65536;
        if tx_bytes.len() > MAX_TX_SIZE {
            return ValidationResult {
                valid: false,
                error_type: Some("SizeError".to_string()),
                error_message: Some(format!(
                    "Transaction size {} exceeds limit {}",
                    tx_bytes.len(),
                    MAX_TX_SIZE
                )),
            };
        }

        // 3. 交易最小长度验证
        // version(1) + nonce(4) + system_fee(8) + network_fee(8) + valid_until_block(4) + signers(至少1) + attributes(至少1) + script(至少1) + witnesses(至少1) = 至少 28 字节
        if tx_bytes.len() < 28 {
            return ValidationResult {
                valid: false,
                error_type: Some("SizeError".to_string()),
                error_message: Some(format!(
                    "Transaction too small: {} bytes (minimum 28)",
                    tx_bytes.len()
                )),
            };
        }

        // 基本验证通过
        ValidationResult {
            valid: true,
            error_type: None,
            error_message: None,
        }
    }

    /// 批量验证区块
    pub fn validate_blocks(
        &self,
        tests: &[BlockValidationTest],
    ) -> HashMap<String, ValidationResult> {
        tests
            .iter()
            .map(|test| {
                let result = self.validate_block(&test.block_hex);
                (test.test_id.clone(), result)
            })
            .collect()
    }

    /// 批量验证交易
    pub fn validate_transactions(
        &self,
        tests: &[TransactionValidationTest],
    ) -> HashMap<String, ValidationResult> {
        tests
            .iter()
            .map(|test| {
                let result = self.validate_transaction(&test.tx_hex);
                (test.test_id.clone(), result)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_creation() {
        let settings = ProtocolSettings::mainnet();
        let _validator = RustValidator::new(settings);
    }
}
