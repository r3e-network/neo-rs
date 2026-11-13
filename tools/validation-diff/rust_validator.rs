//! Strict block/transaction validation for cross-implementation differential tests.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use neo_io::{MemoryReader, Serializable};
use neo_json::JToken;
use neo_ledger::verify_transaction_state_independent;
use neo_primitives::{Verifiable, VerifyResult};
use neo_protocol::payloads::{Block, Transaction};
use neo_protocol::ProtocolSettings;
use neo_rpc::client::models::{RpcBlock, RpcTransaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
}

impl ValidationResult {
    fn success() -> Self {
        Self {
            valid: true,
            error_type: None,
            error_message: None,
        }
    }

    fn failure(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            valid: false,
            error_type: Some(kind.into()),
            error_message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockValidationTest {
    pub test_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_json: Option<Value>,
    pub expected_valid: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionValidationTest {
    pub test_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_json: Option<Value>,
    pub expected_valid: bool,
    pub description: String,
}

enum Input {
    Binary(Vec<u8>),
    RpcJson(Value),
}

pub struct RustValidator {
    protocol_settings: ProtocolSettings,
}

impl RustValidator {
    pub fn new(protocol_settings: ProtocolSettings) -> Self {
        Self { protocol_settings }
    }

    /// Validates a strict Neo wire block supplied as hex (optional `0x`) or Base64.
    #[cfg(test)]
    pub fn validate_block(&self, encoded: &str) -> ValidationResult {
        match decode_binary(encoded, "block") {
            Ok(bytes) => self.validate_block_bytes(&bytes),
            Err(error) => error,
        }
    }

    /// Validates a strict Neo wire transaction supplied as hex or Base64.
    #[cfg(test)]
    pub fn validate_transaction(&self, encoded: &str) -> ValidationResult {
        match decode_binary(encoded, "transaction") {
            Ok(bytes) => self.validate_transaction_bytes(&bytes),
            Err(error) => error,
        }
    }

    fn validate_block_test(&self, test: &BlockValidationTest) -> ValidationResult {
        let sources = [
            test.block_hex.is_some(),
            test.block_base64.is_some(),
            test.block_json.is_some(),
        ];
        if sources.into_iter().filter(|present| *present).count() != 1 {
            return ValidationResult::failure(
                "InputError",
                "exactly one of block_hex, block_base64 or block_json is required",
            );
        }
        let input = if let Some(encoded) = &test.block_hex {
            decode_binary(encoded, "block").map(Input::Binary)
        } else if let Some(encoded) = &test.block_base64 {
            decode_base64(encoded, "block").map(Input::Binary)
        } else {
            Ok(Input::RpcJson(
                test.block_json.clone().expect("one selected input"),
            ))
        };
        match input {
            Ok(Input::Binary(bytes)) => self.validate_block_bytes(&bytes),
            Ok(Input::RpcJson(value)) => self.validate_block_json(&value),
            Err(error) => error,
        }
    }

    fn validate_transaction_test(&self, test: &TransactionValidationTest) -> ValidationResult {
        let sources = [
            test.tx_hex.is_some(),
            test.tx_base64.is_some(),
            test.tx_json.is_some(),
        ];
        if sources.into_iter().filter(|present| *present).count() != 1 {
            return ValidationResult::failure(
                "InputError",
                "exactly one of tx_hex, tx_base64 or tx_json is required",
            );
        }
        let input = if let Some(encoded) = &test.tx_hex {
            decode_binary(encoded, "transaction").map(Input::Binary)
        } else if let Some(encoded) = &test.tx_base64 {
            decode_base64(encoded, "transaction").map(Input::Binary)
        } else {
            Ok(Input::RpcJson(
                test.tx_json.clone().expect("one selected input"),
            ))
        };
        match input {
            Ok(Input::Binary(bytes)) => self.validate_transaction_bytes(&bytes),
            Ok(Input::RpcJson(value)) => self.validate_transaction_json(&value),
            Err(error) => error,
        }
    }

    fn validate_block_bytes(&self, bytes: &[u8]) -> ValidationResult {
        let mut reader = MemoryReader::new(bytes);
        match <Block as Serializable>::deserialize(&mut reader) {
            Ok(block) if reader.remaining() == 0 => validate_block_structure(&block),
            Ok(_) => ValidationResult::failure("TrailingData", "block has trailing wire bytes"),
            Err(error) => ValidationResult::failure("DecodeError", error.to_string()),
        }
    }

    fn validate_transaction_bytes(&self, bytes: &[u8]) -> ValidationResult {
        let mut reader = MemoryReader::new(bytes);
        match <Transaction as Serializable>::deserialize(&mut reader) {
            Ok(transaction) if reader.remaining() == 0 => {
                self.validate_transaction_structure(&transaction)
            }
            Ok(_) => {
                ValidationResult::failure("TrailingData", "transaction has trailing wire bytes")
            }
            Err(error) => ValidationResult::failure("DecodeError", error.to_string()),
        }
    }

    fn validate_block_json(&self, value: &Value) -> ValidationResult {
        match parse_rpc_json(value)
            .and_then(|object| RpcBlock::from_json(&object, &self.protocol_settings))
        {
            Ok(rpc) => validate_block_structure(&rpc.block),
            Err(error) => ValidationResult::failure("JsonError", error),
        }
    }

    fn validate_transaction_json(&self, value: &Value) -> ValidationResult {
        match parse_rpc_json(value)
            .and_then(|object| RpcTransaction::from_json(&object, &self.protocol_settings))
        {
            Ok(rpc) => self.validate_transaction_structure(&rpc.transaction),
            Err(error) => ValidationResult::failure("JsonError", error),
        }
    }

    fn validate_transaction_structure(&self, transaction: &Transaction) -> ValidationResult {
        if !transaction.verify() {
            return ValidationResult::failure(
                "StructuralError",
                "transaction fields, signers or witnesses are structurally invalid",
            );
        }
        let result = verify_transaction_state_independent(transaction, &self.protocol_settings);
        if result == VerifyResult::Succeed {
            ValidationResult::success()
        } else {
            ValidationResult::failure(
                "VerificationError",
                format!("state-independent verification returned {result:?}"),
            )
        }
    }

    /// Batch-validates blocks without aborting the differential run on bad vectors.
    pub fn validate_blocks(
        &self,
        tests: &[BlockValidationTest],
    ) -> HashMap<String, ValidationResult> {
        tests
            .iter()
            .map(|test| (test.test_id.clone(), self.validate_block_test(test)))
            .collect()
    }

    /// Batch-validates transactions without aborting on bad vectors.
    pub fn validate_transactions(
        &self,
        tests: &[TransactionValidationTest],
    ) -> HashMap<String, ValidationResult> {
        tests
            .iter()
            .map(|test| (test.test_id.clone(), self.validate_transaction_test(test)))
            .collect()
    }
}

fn validate_block_structure(block: &Block) -> ValidationResult {
    if block.verify() {
        ValidationResult::success()
    } else {
        ValidationResult::failure(
            "StructuralError",
            "block version, Merkle root or transaction set is invalid",
        )
    }
}

fn parse_rpc_json(value: &Value) -> Result<neo_json::JObject, String> {
    let text = serde_json::to_string(value).map_err(|error| error.to_string())?;
    JToken::parse(&text, 128)
        .map_err(|error| error.to_string())?
        .as_object()
        .cloned()
        .ok_or_else(|| "RPC payload must be a JSON object".to_owned())
}

fn decode_base64(encoded: &str, label: &str) -> Result<Vec<u8>, ValidationResult> {
    BASE64.decode(encoded.trim()).map_err(|error| {
        ValidationResult::failure(
            "DecodeError",
            format!("failed to decode {label} Base64: {error}"),
        )
    })
}

fn decode_binary(encoded: &str, label: &str) -> Result<Vec<u8>, ValidationResult> {
    let trimmed = encoded.trim();
    let hexadecimal = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hexadecimal.len().is_multiple_of(2)
        && hexadecimal.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return hex::decode(hexadecimal).map_err(|error| {
            ValidationResult::failure(
                "DecodeError",
                format!("failed to decode {label} hex: {error}"),
            )
        });
    }
    decode_base64(trimmed, label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::SerializableExtensions;
    use neo_primitives::WitnessScope;
    use neo_protocol::payloads::Signer;
    use neo_protocol::witness::Witness;
    use neo_vm::OpCode;

    fn valid_block() -> Block {
        neo_ledger::create_genesis_block(&ProtocolSettings::mainnet())
    }

    fn valid_transaction() -> Transaction {
        let witness =
            Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
        let signer = Signer::new(witness.script_hash(), WitnessScope::NONE);
        let mut transaction = Transaction::new();
        transaction.set_nonce(7);
        transaction.set_valid_until_block(100);
        transaction.set_script(vec![OpCode::RET.byte()]);
        transaction.set_signers(vec![signer]);
        transaction.set_witnesses(vec![witness]);
        transaction
    }

    fn block_test(encoded: String) -> BlockValidationTest {
        BlockValidationTest {
            test_id: "block".to_owned(),
            block_hex: Some(encoded),
            block_base64: None,
            block_json: None,
            expected_valid: true,
            description: "strict wire block".to_owned(),
        }
    }

    #[test]
    fn hex_prefixed_and_base64_wire_inputs_validate_the_same_block() {
        let validator = RustValidator::new(ProtocolSettings::mainnet());
        let bytes = valid_block().to_array().expect("serialize block");
        let hex_result =
            validator.validate_blocks(&[block_test(format!("0x{}", hex::encode(&bytes)))]);
        let base64_result = validator.validate_blocks(&[BlockValidationTest {
            block_hex: None,
            block_base64: Some(BASE64.encode(bytes)),
            ..block_test(String::new())
        }]);
        assert!(hex_result["block"].valid);
        assert!(base64_result["block"].valid);
    }

    #[test]
    fn rpc_json_block_and_transaction_take_the_real_validation_paths() {
        let settings = ProtocolSettings::mainnet();
        let validator = RustValidator::new(settings.clone());
        let block = RpcBlock {
            block: valid_block(),
            confirmations: 1,
            next_block_hash: None,
        }
        .to_json(&settings)
        .to_string();
        let block_json = serde_json::from_str(&block).expect("block JSON");
        assert!(validator.validate_block_json(&block_json).valid);

        let transaction = RpcTransaction {
            transaction: valid_transaction(),
            block_hash: None,
            confirmations: None,
            block_time: None,
            vm_state: None,
        }
        .to_json(&settings)
        .to_string();
        let transaction_json = serde_json::from_str(&transaction).expect("transaction JSON");
        assert!(validator.validate_transaction_json(&transaction_json).valid);
    }

    #[test]
    fn strict_wire_decoder_rejects_trailing_bytes() {
        let validator = RustValidator::new(ProtocolSettings::mainnet());
        let mut bytes = valid_block().to_array().expect("serialize block");
        bytes.push(0);
        let result = validator.validate_block(&hex::encode(bytes));
        assert!(!result.valid);
        assert_eq!(result.error_type.as_deref(), Some("TrailingData"));
    }

    #[test]
    fn transaction_runs_real_witness_verification() {
        let validator = RustValidator::new(ProtocolSettings::mainnet());
        let bytes = valid_transaction()
            .to_array()
            .expect("serialize transaction");
        assert!(validator.validate_transaction(&hex::encode(bytes)).valid);
        let mut invalid = valid_transaction();
        invalid.set_witnesses(Vec::new());
        let result = validator.validate_transaction(&hex::encode(invalid.to_array().unwrap()));
        assert!(!result.valid);
    }

    #[test]
    fn vectors_require_exactly_one_input_encoding() {
        let validator = RustValidator::new(ProtocolSettings::mainnet());
        let result = validator.validate_blocks(&[BlockValidationTest {
            block_hex: None,
            ..block_test(String::new())
        }]);
        assert_eq!(result["block"].error_type.as_deref(), Some("InputError"));
    }
}
