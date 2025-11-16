// Copyright (C) 2015-2025 The Neo Project.
//
// utility.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use base64::{engine::general_purpose, Engine as _};
use neo_core::network::p2p::payloads::{
    conflicts::Conflicts, not_valid_before::NotValidBefore, notary_assisted::NotaryAssisted,
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode,
};
use neo_core::{
    Block, BlockHeader, Contract, ECPoint, KeyPair, NativeContract, ProtocolSettings, Signer,
    Transaction, TransactionAttribute, UInt160, UInt256, Wallet, Witness, WitnessCondition,
};
use neo_json::{JArray, JObject, JToken};
use neo_vm::stack_item::InteropInterface;
use neo_vm::{Script, StackItem};
use num_bigint::BigInt;
use serde_json::Value as JsonValue;
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Utility functions for RPC client
/// Matches C# Utility class
pub struct Utility;

impl Utility {
    /// Converts a decimal to a fraction
    /// Matches C# Fraction
    fn fraction(d: f64) -> (BigInt, BigInt) {
        // Convert decimal to rational approximation
        // This is a simplified version - proper implementation would handle all cases
        let whole = d.trunc() as i64;
        let decimal = d.fract();

        // Find denominator (simplified approach)
        let denominator = BigInt::from(10_000_000);
        let numerator = BigInt::from((d * 10_000_000.0) as i64);

        (numerator, denominator)
    }

    /// Converts a JToken to a script hash
    /// Matches C# ToScriptHash extension
    pub fn to_script_hash(
        value: &JToken,
        protocol_settings: &ProtocolSettings,
    ) -> Result<UInt160, String> {
        let address_or_script_hash = value.as_string().ok_or("Value is not a string")?;

        if address_or_script_hash.len() < 40 {
            UInt160::from_address(address_or_script_hash, protocol_settings.address_version)
                .map_err(|e| e.to_string())
        } else {
            UInt160::parse(address_or_script_hash).map_err(|e| e.to_string())
        }
    }

    /// Converts an address or script hash string to script hash string
    /// Matches C# AsScriptHash extension
    pub fn as_script_hash(address_or_script_hash: &str) -> String {
        // Check native contracts
        for native in NativeContract::all() {
            if address_or_script_hash.eq_ignore_ascii_case(native.name())
                || address_or_script_hash == native.id().to_string()
            {
                return native.hash().to_string();
            }
        }

        if address_or_script_hash.len() < 40 {
            address_or_script_hash.to_string()
        } else {
            match UInt160::parse(address_or_script_hash) {
                Ok(hash) => hash.to_string(),
                Err(_) => address_or_script_hash.to_string(),
            }
        }
    }

    /// Parse WIF or private key hex string to KeyPair
    /// Matches C# GetKeyPair
    pub fn get_key_pair(key: &str) -> Result<KeyPair, String> {
        if key.is_empty() {
            return Err("Key cannot be empty".to_string());
        }

        let key = if key.starts_with("0x") {
            &key[2..]
        } else {
            key
        };

        match key.len() {
            52 => {
                // WIF format
                let private_key = Wallet::get_private_key_from_wif(key)?;
                Ok(KeyPair::from_private_key(&private_key)?)
            }
            64 => {
                // Hex private key
                let bytes = hex::decode(key).map_err(|e| e.to_string())?;
                Ok(KeyPair::from_private_key(&bytes)?)
            }
            _ => Err("Invalid key format".to_string()),
        }
    }

    /// Parse address, scripthash or public key string to UInt160
    /// Matches C# GetScriptHash
    pub fn get_script_hash(
        account: &str,
        protocol_settings: &ProtocolSettings,
    ) -> Result<UInt160, String> {
        if account.is_empty() {
            return Err("Account cannot be empty".to_string());
        }

        let account = if account.starts_with("0x") {
            &account[2..]
        } else {
            account
        };

        match account.len() {
            34 => {
                // Address
                UInt160::from_address(account, protocol_settings.address_version)
                    .map_err(|e| e.to_string())
            }
            40 => {
                // Script hash
                UInt160::parse(account).map_err(|e| e.to_string())
            }
            66 => {
                // Public key
                let point = ECPoint::parse(account)?;
                let script = Contract::create_signature_redeem_script(&point);
                Ok(script.to_script_hash())
            }
            _ => Err("Invalid account format".to_string()),
        }
    }

    /// Converts a block to JSON representation.
    pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        let header = &block.header;

        json.insert("hash".to_string(), JToken::String(block.hash().to_string()));
        let block_size =
            header.size() + block.transactions.iter().map(|tx| tx.size()).sum::<usize>();
        json.insert("size".to_string(), JToken::Number(block_size as f64));
        json.insert("version".to_string(), JToken::Number(header.version as f64));
        json.insert(
            "previousblockhash".to_string(),
            JToken::String(header.previous_hash.to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            JToken::String(header.merkle_root.to_string()),
        );
        json.insert("time".to_string(), JToken::Number(header.timestamp as f64));
        json.insert(
            "nonce".to_string(),
            JToken::String(format!("{:016x}", header.nonce)),
        );
        json.insert("index".to_string(), JToken::Number(header.index as f64));
        json.insert(
            "primary".to_string(),
            JToken::Number(header.primary_index as f64),
        );
        json.insert(
            "nextconsensus".to_string(),
            JToken::String(
                header
                    .next_consensus
                    .to_address(protocol_settings.address_version),
            ),
        );
        json.insert(
            "witnesses".to_string(),
            JToken::Array(
                header
                    .witnesses
                    .iter()
                    .map(|w| JToken::Object(witness_to_json(w)))
                    .collect(),
            ),
        );
        json.insert(
            "tx".to_string(),
            JToken::Array(
                block
                    .transactions
                    .iter()
                    .map(|tx| JToken::Object(Self::transaction_to_json(tx, protocol_settings)))
                    .collect(),
            ),
        );

        json
    }

    /// Parses a block header from JSON
    pub fn header_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> Result<BlockHeader, String> {
        let version = json
            .get("version")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'version' field")? as u32;

        let previous_hash = json
            .get("previousblockhash")
            .and_then(|v| v.as_string())
            .and_then(|value| UInt256::parse(&value).ok())
            .ok_or("Missing or invalid 'previousblockhash' field")?;

        let merkle_root = json
            .get("merkleroot")
            .and_then(|v| v.as_string())
            .and_then(|value| UInt256::parse(&value).ok())
            .ok_or("Missing or invalid 'merkleroot' field")?;

        let timestamp = json
            .get("time")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'time' field")? as u64;

        let nonce_token = json
            .get("nonce")
            .ok_or("Missing 'nonce' field for header parsing")?;
        let nonce = parse_nonce_token(nonce_token)?;

        let index = json
            .get("index")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'index' field")? as u32;

        let primary_index = json
            .get("primary")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'primary' field")? as u8;

        let next_consensus_text = json
            .get("nextconsensus")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'nextconsensus' field")?;
        let next_consensus = Self::get_script_hash(next_consensus_text, protocol_settings)
            .map_err(|err| format!("Invalid 'nextconsensus' field in block header: {err}"))?;

        let witnesses = json
            .get("witnesses")
            .and_then(|token| token.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| {
                        let obj = entry
                            .as_object()
                            .ok_or_else(|| "Witness entry must be an object".to_string())?;
                        Self::witness_from_json(obj)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_else(|| Ok(Vec::new()))?;

        Ok(BlockHeader::new(
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses,
        ))
    }

    /// Converts JSON to a block
    /// Matches C# BlockFromJson
    pub fn block_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Block, String> {
        let header = Self::header_from_json(json, protocol_settings)?;

        let transactions = json
            .get("tx")
            .and_then(|token| token.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| {
                        let obj = entry
                            .as_object()
                            .ok_or_else(|| "Transaction entry must be an object".to_string())?;
                        Self::transaction_from_json(obj, protocol_settings)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_else(|| Ok(Vec::new()))?;

        Ok(Block::new(header, transactions))
    }

    /// Converts a transaction to JSON
    /// Matches C# TransactionToJson
    pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();

        json.insert("hash".to_string(), JToken::String(tx.hash().to_string()));
        json.insert("size".to_string(), JToken::Number(tx.size() as f64));
        json.insert("version".to_string(), JToken::Number(tx.version as f64));
        json.insert("nonce".to_string(), JToken::Number(tx.nonce as f64));
        json.insert(
            "sender".to_string(),
            JToken::String(tx.sender().to_address(protocol_settings.address_version)),
        );
        json.insert(
            "sysfee".to_string(),
            JToken::String(tx.system_fee.to_string()),
        );
        json.insert(
            "netfee".to_string(),
            JToken::String(tx.network_fee.to_string()),
        );
        json.insert(
            "validuntilblock".to_string(),
            JToken::Number(tx.valid_until_block as f64),
        );

        // Add signers
        json.insert(
            "signers".to_string(),
            JToken::Array(
                tx.signers()
                    .iter()
                    .map(|s| JToken::Object(signer_to_json(s, protocol_settings)))
                    .collect(),
            ),
        );

        // Add attributes
        json.insert(
            "attributes".to_string(),
            JToken::Array(
                tx.attributes()
                    .iter()
                    .map(|a| JToken::Object(attribute_to_json(a)))
                    .collect(),
            ),
        );

        // Add script
        json.insert(
            "script".to_string(),
            JToken::String(general_purpose::STANDARD.encode(&tx.script())),
        );

        // Add witnesses
        json.insert(
            "witnesses".to_string(),
            JToken::Array(
                tx.witnesses()
                    .iter()
                    .map(|w| JToken::Object(witness_to_json(w)))
                    .collect(),
            ),
        );

        json
    }

    /// Converts JSON to a transaction
    /// Matches C# TransactionFromJson
    pub fn transaction_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Transaction, String> {
        let mut tx = Transaction::new();

        if let Some(version) = json.get("version").and_then(|v| v.as_number()) {
            tx.set_version(version as u8);
        }

        if let Some(nonce_token) = json.get("nonce") {
            let nonce = if let Some(number) = nonce_token.as_number() {
                number as u32
            } else if let Some(text) = nonce_token.as_string() {
                text.parse::<u32>()
                    .map_err(|err| format!("Invalid nonce value: {err}"))?
            } else {
                return Err("Invalid 'nonce' field".to_string());
            };
            tx.set_nonce(nonce);
        }

        if let Some(sysfee_token) = json.get("sysfee") {
            let system_fee = parse_i64_token(sysfee_token, "sysfee")?;
            tx.set_system_fee(system_fee);
        }

        if let Some(netfee_token) = json.get("netfee") {
            let network_fee = parse_i64_token(netfee_token, "netfee")?;
            tx.set_network_fee(network_fee);
        }

        if let Some(valid_token) = json.get("validuntilblock") {
            let height = parse_u32_token(valid_token, "validuntilblock")?;
            tx.set_valid_until_block(height);
        }

        if let Some(signers_token) = json.get("signers").and_then(|t| t.as_array()) {
            let mut parsed_signers = Vec::with_capacity(signers_token.len());
            for entry in signers_token {
                let signer_json = jtoken_to_serde(entry)?;
                parsed_signers.push(
                    Signer::from_json(&signer_json)
                        .map_err(|err| format!("Invalid signer entry: {err}"))?,
                );
            }
            tx.set_signers(parsed_signers);
        }

        if let Some(attributes_token) = json.get("attributes").and_then(|t| t.as_array()) {
            let mut attributes = Vec::with_capacity(attributes_token.len());
            for entry in attributes_token {
                let attr_obj = entry
                    .as_object()
                    .ok_or_else(|| "Transaction attribute must be an object".to_string())?;
                attributes.push(attribute_from_json(attr_obj)?);
            }
            tx.set_attributes(attributes);
        }

        if let Some(script_token) = json.get("script") {
            let script_str = script_token
                .as_string()
                .ok_or("Missing or invalid 'script' field")?;
            let script_bytes = general_purpose::STANDARD
                .decode(script_str.as_bytes())
                .map_err(|err| format!("Invalid 'script' value: {err}"))?;
            tx.set_script(script_bytes);
        }

        if let Some(witnesses_token) = json.get("witnesses").and_then(|t| t.as_array()) {
            let mut witnesses = Vec::with_capacity(witnesses_token.len());
            for entry in witnesses_token {
                let witness_obj = entry
                    .as_object()
                    .ok_or_else(|| "Witness entry must be an object".to_string())?;
                witnesses.push(Self::witness_from_json(witness_obj)?);
            }
            tx.set_witnesses(witnesses);
        }

        Ok(tx)
    }

    /// Converts a `neo-json` representation of a stack item back into a VM stack item.
    pub fn stack_item_from_json(json: &JObject) -> Result<StackItem, String> {
        let item_type = json
            .get("type")
            .and_then(|v| v.as_string())
            .ok_or("StackItem entry missing 'type' field")?;

        match item_type {
            "Any" => Ok(StackItem::null()),
            "Boolean" => {
                let value = json
                    .get("value")
                    .map(|token| token.as_boolean())
                    .ok_or("Boolean stack item missing 'value' field")?;
                Ok(StackItem::from_bool(value))
            }
            "Integer" => {
                let value_token = json
                    .get("value")
                    .ok_or("Integer stack item missing 'value' field")?;
                let text = value_token
                    .as_string()
                    .ok_or("Integer stack item value must be a string")?;
                let integer = BigInt::parse_bytes(text.as_bytes(), 10)
                    .ok_or("Invalid integer stack item value")?;
                Ok(StackItem::from_int(integer))
            }
            "ByteString" => {
                let value_token = json
                    .get("value")
                    .ok_or("ByteString stack item missing 'value' field")?;
                let data = parse_base64_token(value_token, "value")?;
                Ok(StackItem::from_byte_string(data))
            }
            "Buffer" => {
                let value_token = json
                    .get("value")
                    .ok_or("Buffer stack item missing 'value' field")?;
                let data = parse_base64_token(value_token, "value")?;
                Ok(StackItem::from_buffer(data))
            }
            "Array" => {
                let values = json
                    .get("value")
                    .and_then(|token| token.as_array())
                    .ok_or("Array stack item missing 'value' array")?;
                let mut items = Vec::with_capacity(values.len());
                for value in values.iter() {
                    let token = value.as_ref().ok_or("Array entries must be objects")?;
                    let obj = token.as_object().ok_or("Array entries must be objects")?;
                    items.push(Self::stack_item_from_json(obj)?);
                }
                Ok(StackItem::from_array(items))
            }
            "Struct" => {
                let values = json
                    .get("value")
                    .and_then(|token| token.as_array())
                    .ok_or("Struct stack item missing 'value' array")?;
                let mut items = Vec::with_capacity(values.len());
                for value in values.iter() {
                    let token = value.as_ref().ok_or("Struct entries must be objects")?;
                    let obj = token.as_object().ok_or("Struct entries must be objects")?;
                    items.push(Self::stack_item_from_json(obj)?);
                }
                Ok(StackItem::from_struct(items))
            }
            "Map" => {
                let entries = json
                    .get("value")
                    .and_then(|token| token.as_array())
                    .ok_or("Map stack item missing 'value' array")?;
                let mut map = BTreeMap::new();
                for entry in entries.iter() {
                    let token = entry.as_ref().ok_or("Map entries must be objects")?;
                    let obj = token.as_object().ok_or("Map entries must be objects")?;
                    let key_obj = obj
                        .get("key")
                        .and_then(|token| token.as_ref())
                        .and_then(|token| token.as_object())
                        .ok_or("Map entry missing 'key' object")?;
                    let value_obj = obj
                        .get("value")
                        .and_then(|token| token.as_ref())
                        .and_then(|token| token.as_object())
                        .ok_or("Map entry missing 'value' object")?;
                    let key = Self::stack_item_from_json(key_obj)?;
                    let value = Self::stack_item_from_json(value_obj)?;
                    map.insert(key, value);
                }
                Ok(StackItem::from_map(map))
            }
            "Pointer" => {
                let index_token = json
                    .get("value")
                    .ok_or("Pointer stack item missing 'value' field")?;
                let index = parse_u32_token(index_token, "value")? as usize;
                let script = Arc::new(Script::new_relaxed(Vec::new()));
                Ok(StackItem::from_pointer(script, index))
            }
            "InteropInterface" => {
                let payload = json
                    .get("value")
                    .and_then(|token| token.as_object())
                    .ok_or("InteropInterface missing 'value' object")?;
                let serde_payload = jobject_to_serde(payload)?;
                Ok(StackItem::from_interface(JsonInteropInterface::new(
                    serde_payload,
                )))
            }
            other => {
                // Treat unknown types as null to match the C# default branch behaviour.
                Err(format!(
                    "Unsupported stack item type '{other}' in JSON payload"
                ))
            }
        }
    }

    /// Creates a witness from JSON (invocation/verification scripts encoded as base64).
    pub fn witness_from_json(json: &JObject) -> Result<Witness, String> {
        let invocation = json
            .get("invocation")
            .and_then(|value| value.as_string())
            .ok_or("Missing 'invocation' field")?;
        let verification = json
            .get("verification")
            .and_then(|value| value.as_string())
            .ok_or("Missing 'verification' field")?;

        let invocation_bytes = general_purpose::STANDARD
            .decode(invocation.as_bytes())
            .map_err(|err| format!("Invalid invocation script: {err}"))?;
        let verification_bytes = general_purpose::STANDARD
            .decode(verification.as_bytes())
            .map_err(|err| format!("Invalid verification script: {err}"))?;

        Ok(Witness::new_with_scripts(
            invocation_bytes,
            verification_bytes,
        ))
    }
}

#[derive(Debug)]
struct JsonInteropInterface {
    payload: JsonValue,
}

impl JsonInteropInterface {
    fn new(payload: JsonValue) -> Self {
        Self { payload }
    }
}

impl InteropInterface for JsonInteropInterface {
    fn interface_type(&self) -> &str {
        "json"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn attribute_from_json(json: &JObject) -> Result<TransactionAttribute, String> {
    let attr_type = json
        .get("type")
        .and_then(|v| v.as_string())
        .ok_or("Transaction attribute missing 'type' field")?;

    match attr_type {
        "HighPriority" => Ok(TransactionAttribute::HighPriority),
        "NotValidBefore" => {
            let height_token = json
                .get("height")
                .ok_or("NotValidBefore attribute missing 'height' field")?;
            let height = parse_u32_token(height_token, "height")?;
            Ok(TransactionAttribute::NotValidBefore(NotValidBefore::new(
                height,
            )))
        }
        "Conflicts" => {
            let hash_str = json
                .get("hash")
                .and_then(|v| v.as_string())
                .ok_or("Conflicts attribute missing 'hash' field")?;
            let hash = UInt256::parse(&hash_str)
                .map_err(|err| format!("Invalid conflicts hash: {err}"))?;
            Ok(TransactionAttribute::Conflicts(Conflicts::new(hash)))
        }
        "NotaryAssisted" => {
            let nkeys_token = json
                .get("nkeys")
                .ok_or("NotaryAssisted attribute missing 'nkeys' field")?;
            let nkeys = parse_u32_token(nkeys_token, "nkeys")?;
            Ok(TransactionAttribute::NotaryAssisted(NotaryAssisted::new(
                nkeys as u8,
            )))
        }
        "OracleResponse" => {
            let id_token = json
                .get("id")
                .ok_or("OracleResponse attribute missing 'id' field")?;
            let id = parse_u64_token(id_token, "id")?;
            let code_token = json
                .get("code")
                .ok_or("OracleResponse attribute missing 'code' field")?;
            let code = parse_oracle_response_code(code_token)?;
            let result_token = json
                .get("result")
                .ok_or("OracleResponse attribute missing 'result' field")?;
            let result = parse_base64_token(result_token, "result")?;
            Ok(TransactionAttribute::OracleResponse(OracleResponse::new(
                id, code, result,
            )))
        }
        other => Err(format!(
            "Unsupported transaction attribute type '{other}' in RPC payload"
        )),
    }
}

fn parse_oracle_response_code(token: &JToken) -> Result<OracleResponseCode, String> {
    if let Some(text) = token.as_string() {
        match text {
            "Success" => Ok(OracleResponseCode::Success),
            "ProtocolNotSupported" => Ok(OracleResponseCode::ProtocolNotSupported),
            "ConsensusUnreachable" => Ok(OracleResponseCode::ConsensusUnreachable),
            "NotFound" => Ok(OracleResponseCode::NotFound),
            "Timeout" => Ok(OracleResponseCode::Timeout),
            "Forbidden" => Ok(OracleResponseCode::Forbidden),
            "ResponseTooLarge" => Ok(OracleResponseCode::ResponseTooLarge),
            "InsufficientFunds" => Ok(OracleResponseCode::InsufficientFunds),
            "ContentTypeNotSupported" => Ok(OracleResponseCode::ContentTypeNotSupported),
            "Error" => Ok(OracleResponseCode::Error),
            other => {
                let normalized = other.trim_start_matches("0x");
                let value = u8::from_str_radix(normalized, 16)
                    .map_err(|err| format!("Invalid oracle response code '{other}': {err}"))?;
                OracleResponseCode::from_byte(value).ok_or_else(|| {
                    format!("Unknown oracle response code value '{other}' in RPC payload")
                })
            }
        }
    } else if let Some(number) = token.as_number() {
        let value = number as u8;
        OracleResponseCode::from_byte(value)
            .ok_or_else(|| format!("Unknown oracle response code value '{value}' in RPC payload"))
    } else {
        Err("OracleResponse attribute 'code' must be a string or number".to_string())
    }
}

fn parse_base64_token(token: &JToken, field: &str) -> Result<Vec<u8>, String> {
    let text = token
        .as_string()
        .ok_or_else(|| format!("Field '{field}' must be a base64 string"))?;
    general_purpose::STANDARD
        .decode(text.as_bytes())
        .map_err(|err| format!("Invalid base64 data in '{field}': {err}"))
}

fn parse_u32_token(token: &JToken, field: &str) -> Result<u32, String> {
    if let Some(number) = token.as_number() {
        Ok(number as u32)
    } else if let Some(text) = token.as_string() {
        text.parse::<u32>()
            .map_err(|err| format!("Invalid unsigned integer for '{field}': {err}"))
    } else {
        Err(format!("Field '{field}' must be a number"))
    }
}

fn parse_u64_token(token: &JToken, field: &str) -> Result<u64, String> {
    if let Some(number) = token.as_number() {
        Ok(number as u64)
    } else if let Some(text) = token.as_string() {
        text.parse::<u64>()
            .map_err(|err| format!("Invalid unsigned integer for '{field}': {err}"))
    } else {
        Err(format!("Field '{field}' must be a number"))
    }
}

fn parse_i64_token(token: &JToken, field: &str) -> Result<i64, String> {
    if let Some(number) = token.as_number() {
        Ok(number as i64)
    } else if let Some(text) = token.as_string() {
        text.parse::<i64>()
            .map_err(|err| format!("Invalid signed integer for '{field}': {err}"))
    } else {
        Err(format!("Field '{field}' must be a number"))
    }
}

fn parse_nonce_token(token: &JToken) -> Result<u64, String> {
    if let Some(text) = token.as_string() {
        let value = text.trim_start_matches("0x");
        u64::from_str_radix(value, 16)
            .map_err(|err| format!("Invalid nonce hex string '{text}': {err}"))
    } else if let Some(number) = token.as_number() {
        Ok(number as u64)
    } else {
        Err("Nonce value must be a hex string or number".to_string())
    }
}

fn jtoken_to_serde(token: &JToken) -> Result<JsonValue, String> {
    serde_json::from_str(&token.to_string()).map_err(|err| err.to_string())
}

fn jobject_to_serde(obj: &JObject) -> Result<JsonValue, String> {
    serde_json::from_str(&obj.to_string()).map_err(|err| err.to_string())
}

/// Public wrappers matching the historical `crate::utility::function` style.
pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
    Utility::block_to_json(block, protocol_settings)
}

pub fn block_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> Result<Block, String> {
    Utility::block_from_json(json, protocol_settings)
}

pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
    Utility::transaction_to_json(tx, protocol_settings)
}

pub fn transaction_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> Result<Transaction, String> {
    Utility::transaction_from_json(json, protocol_settings)
}

pub fn stack_item_from_json(json: &JObject) -> Result<StackItem, String> {
    Utility::stack_item_from_json(json)
}

pub fn witness_from_json(json: &JObject) -> Result<Witness, String> {
    Utility::witness_from_json(json)
}

// Helper functions for JSON conversion

fn witness_to_json(witness: &neo_core::Witness) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "invocation".to_string(),
        JToken::String(general_purpose::STANDARD.encode(&witness.invocation_script)),
    );
    json.insert(
        "verification".to_string(),
        JToken::String(general_purpose::STANDARD.encode(&witness.verification_script)),
    );
    json
}

fn signer_to_json(signer: &neo_core::Signer, protocol_settings: &ProtocolSettings) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "account".to_string(),
        JToken::String(signer.account.to_address(protocol_settings.address_version)),
    );
    json.insert(
        "scopes".to_string(),
        JToken::String(signer.scopes.to_string()),
    );

    if !signer.allowed_contracts.is_empty() {
        json.insert(
            "allowedcontracts".to_string(),
            JToken::Array(
                signer
                    .allowed_contracts
                    .iter()
                    .map(|c| JToken::String(c.to_string()))
                    .collect(),
            ),
        );
    }

    if !signer.allowed_groups.is_empty() {
        json.insert(
            "allowedgroups".to_string(),
            JToken::Array(
                signer
                    .allowed_groups
                    .iter()
                    .map(|g| JToken::String(g.to_string()))
                    .collect(),
            ),
        );
    }

    if !signer.rules.is_empty() {
        json.insert(
            "rules".to_string(),
            JToken::Array(
                signer
                    .rules
                    .iter()
                    .map(|r| JToken::Object(rule_to_json(r)))
                    .collect(),
            ),
        );
    }

    json
}

fn attribute_to_json(attr: &neo_core::TransactionAttribute) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "type".to_string(),
        JToken::String(attr.get_type().to_string()),
    );
    // Add attribute-specific data based on type
    json
}

fn rule_to_json(rule: &neo_core::WitnessRule) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "action".to_string(),
        JToken::String(rule.action.to_string()),
    );
    json.insert(
        "condition".to_string(),
        JToken::Object(condition_to_json(&rule.condition)),
    );
    json
}

fn condition_to_json(condition: &WitnessCondition) -> JObject {
    use neo_core::WitnessCondition as WC;

    let mut json = JObject::new();
    json.insert(
        "type".to_string(),
        JToken::String(condition.condition_type().to_string()),
    );

    match condition {
        WC::Boolean { value } => {
            json.insert("expression".to_string(), JToken::Boolean(*value));
        }
        WC::Not { condition } => {
            json.insert(
                "expression".to_string(),
                JToken::Object(condition_to_json(condition)),
            );
        }
        WC::And { conditions } | WC::Or { conditions } => {
            let expressions = conditions
                .iter()
                .map(|c| JToken::Object(condition_to_json(c)))
                .collect::<Vec<_>>();
            json.insert(
                "expressions".to_string(),
                JToken::Array(JArray::from(expressions)),
            );
        }
        WC::ScriptHash { hash } | WC::CalledByContract { hash } => {
            json.insert("hash".to_string(), JToken::String(hash.to_string()));
        }
        WC::Group { group } | WC::CalledByGroup { group } => {
            json.insert("group".to_string(), JToken::String(hex::encode(group)));
        }
        WC::CalledByEntry => { /* no additional properties */ }
    }

    json
}
