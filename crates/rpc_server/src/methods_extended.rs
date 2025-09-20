//! Extended RPC methods for Neo compatibility
//!
//! These implementations currently provide basic stubs for methods that are
//! still being wired into the Rust node. Methods that are not yet available
//! return an RPC error so the client can see that the functionality is
//! pending.

use crate::methods::{Error, Result, RpcMethods};
use base64::{engine::general_purpose, Engine};
use hex;
use neo_core::{Transaction, UInt160, UInt256};
use neo_smart_contract::contract_state::ContractState;
use serde_json::{json, Value};
use std::str::FromStr;
use tracing::warn;

impl RpcMethods {
    pub(crate) fn contract_state_to_json(&self, state: &ContractState) -> Result<Value> {
        const NEF_MAGIC: u32 = 0x3346454E; // "NEF3" magic value

        let hash_hex = state.hash.to_string();

        let tokens: Vec<Value> = state
            .nef
            .tokens
            .iter()
            .map(|token| {
                json!({
                    "hash": token.hash.to_string(),
                    "method": token.method,
                    "paramcount": token.parameters_count,
                    "hasreturnvalue": token.has_return_value,
                    "callflags": token.call_flags.0,
                })
            })
            .collect();

        let manifest_json = state
            .manifest
            .to_json()
            .map_err(|e| Error::from(format!("Failed to serialize manifest: {}", e)))?;

        Ok(json!({
            "id": state.id,
            "updatecounter": state.update_counter,
            "hash": hash_hex,
            "nef": {
                "magic": NEF_MAGIC,
                "compiler": state.nef.compiler,
                "source": state.nef.source,
                "tokens": tokens,
                "script": general_purpose::STANDARD.encode(&state.nef.script),
                "checksum": state.nef.checksum
            },
            "manifest": manifest_json
        }))
    }

    async fn resolve_contract_hash(&self, identifier: &Value) -> Result<UInt160> {
        if let Some(s) = identifier.as_str() {
            let trimmed = s.trim();
            if let Ok(hash) = Self::parse_uint160(trimmed) {
                return Ok(hash);
            }

            let lower = trimmed.to_ascii_lowercase();
            if let Some(hash_str) = Self::native_contract_hash_by_name(&lower) {
                return Self::parse_uint160(hash_str);
            }

            return Err(format!("Unknown contract name: {}", trimmed).into());
        }

        if let Some(id) = identifier.as_i64() {
            let id = id as i32;
            let contracts = self.ledger.list_native_contracts().await;
            if let Some(state) = contracts.into_iter().find(|c| c.id == id) {
                return Ok(state.hash);
            }
            return Err(format!("Contract not found for id {}", id).into());
        }

        Err("Invalid contract identifier".into())
    }

    fn parse_uint160(value: &str) -> Result<UInt160> {
        let trimmed = value.trim_start_matches("0x");
        UInt160::from_str(trimmed).map_err(|e| format!("Invalid UInt160: {}", e).into())
    }

    fn parse_uint256(value: &str) -> Result<UInt256> {
        let trimmed = value.trim_start_matches("0x");
        UInt256::from_str(trimmed).map_err(|e| format!("Invalid UInt256: {}", e).into())
    }

    fn decode_storage_key(key: &str) -> Result<Vec<u8>> {
        if let Ok(bytes) = general_purpose::STANDARD.decode(key) {
            return Ok(bytes);
        }

        let trimmed = key.trim_start_matches("0x");
        hex::decode(trimmed).map_err(|_| format!("Invalid storage key: {}", key).into())
    }

    fn native_contract_hash_by_name(name: &str) -> Option<&'static str> {
        match name {
            "contractmanagement" => Some("fffdc93764dbaddd97c48f252a53ea4643faa3fd"),
            "neotoken" | "neo" => Some("ef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"),
            "gastoken" | "gas" => Some("d2a4cff31913016155e38e474a2c06d08be276cf"),
            "policy" | "policycontract" => Some("cc5e4edd9f5f8dba8bb65734541df7a1c081c67b"),
            "oracle" | "oraclecontract" => Some("fe924b7cfe89ddd271abaf7210a80a7e11178758"),
            "role" | "rolemanagement" => Some("49cf4e5378ffcd4dec034fd98a174c5491e395e2"),
            "std" | "stdlib" => Some("acce6fd80d44e1796aa0c2c625e9e4e0ce39efc0"),
            "crypto" | "cryptolib" => Some("726cb6e0cd8628a1350a611384688911ab75f51b"),
            "ledger" | "ledgercontract" => Some("da65b600f7124ce6c79950c1772a36403104f2be"),
            _ => None,
        }
    }

    /// Returns an explicit error so callers know this RPC is not available yet.
    pub async fn get_raw_transaction(&self, _params: Value) -> Result<Value> {
        Err(format!("getrawtransaction is not implemented in the current Rust node build").into())
    }

    /// Returns the hashes that are currently stored in the shared mempool.
    pub async fn get_raw_mempool(&self, _params: Value) -> Result<Value> {
        let mempool = self.mempool.read().await;
        let transactions = mempool.get_verified_transactions();
        drop(mempool);

        let mut verified = Vec::with_capacity(transactions.len());
        for tx in transactions {
            match tx.hash() {
                Ok(hash) => verified.push(hash.to_string()),
                Err(e) => warn!("Failed to compute transaction hash: {}", e),
            }
        }

        Ok(json!({
            "height": self.ledger.get_height().await,
            "verified": verified,
            "unverified": []
        }))
    }

    /// Adds a raw transaction to the mempool and broadcasts an inventory message.
    pub async fn send_raw_transaction(&self, params: Value) -> Result<Value> {
        let raw_tx = params
            .get(0)
            .and_then(|v| v.as_str())
            .ok_or("Missing raw transaction parameter")?;

        let tx_bytes = hex::decode(raw_tx).map_err(|_| "Invalid hex format")?;
        let transaction =
            Transaction::from_bytes(&tx_bytes).map_err(|_| "Invalid transaction format")?;
        let was_added = {
            let mempool = self.mempool.write().await;
            match mempool.try_add(transaction.clone(), false) {
                Ok(true) => true,
                Ok(false) => return Ok(json!(false)),
                Err(e) => return Err(e.into()),
            }
        };

        if was_added {
            // TODO: broadcast inventory once networking integration is available.
        }

        Ok(json!(true))
    }

    pub async fn get_storage(&self, params: Value) -> Result<Value> {
        let params_array = params.as_array().ok_or("Invalid parameters format")?;
        if params_array.len() < 2 {
            return Err("Missing storage parameters".into());
        }

        let contract_hash_str = params_array[0]
            .as_str()
            .ok_or("Invalid contract hash format")?;
        let contract_hash = Self::parse_uint160(contract_hash_str)?;
        let hash_bytes = contract_hash.as_bytes();

        let key_str = params_array[1]
            .as_str()
            .ok_or("Invalid storage key format")?;
        let key_bytes = Self::decode_storage_key(key_str)?;

        match self
            .ledger
            .get_raw_storage_value(&hash_bytes, &key_bytes)
            .await?
        {
            Some(value) => Ok(json!(format!("0x{}", hex::encode(value)))),
            None => Ok(Value::Null),
        }
    }

    pub async fn invoke_function(&self, params: Value) -> Result<Value> {
        use neo_vm::{CallFlags, ScriptBuilder};

        let params_array = params.as_array().ok_or("Invalid parameters format")?;
        if params_array.len() < 2 {
            return Err("Missing contract hash or operation".into());
        }

        let contract_identifier = params_array[0].clone();
        let contract_hash = self.resolve_contract_hash(&contract_identifier).await?;

        let operation = params_array[1]
            .as_str()
            .ok_or("Invalid operation format")?
            .to_string();

        let arguments_value = params_array
            .get(2)
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));

        let call_flags = Self::parse_call_flags(params_array.get(3))?;

        let parameters = Self::parse_invoke_arguments(&arguments_value)?;

        let mut builder = ScriptBuilder::new();

        // Push parameters array in reverse order and pack
        for item in parameters.iter().rev() {
            builder
                .emit_push_stack_item(item.clone())
                .map_err(|e| format!("Failed to serialize argument: {e}"))?;
        }
        builder.emit_push_int(parameters.len() as i64);
        builder.emit_pack();

        // Push call flags, method name and contract hash following C# EmitDynamicCall layout
        builder.emit_push_int(call_flags.0 as i64);
        builder.emit_push_string(&operation);
        let hash_bytes = contract_hash.as_bytes();
        builder.emit_push_byte_array(&hash_bytes);
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|e| format!("Failed to emit syscall: {e}"))?;

        let script_hex = hex::encode(builder.to_array());

        Ok(json!({
            "script": script_hex,
            "state": "FAULT",
            "gasconsumed": "0",
            "stack": Vec::<Value>::new(),
            "exception": "invokefunction is not implemented in the current Rust node build",
            "contract": contract_hash.to_string(),
            "operation": operation,
            "arguments": arguments_value
        }))
    }
    fn parse_call_flags(raw: Option<&Value>) -> Result<neo_vm::CallFlags> {
        use neo_vm::CallFlags;

        let Some(value) = raw else {
            return Ok(CallFlags::ALL);
        };

        match value {
            Value::Null => Ok(CallFlags::ALL),
            Value::String(s) => {
                let flag = match s.trim().to_ascii_lowercase().as_str() {
                    "all" => CallFlags::ALL,
                    "none" => CallFlags::NONE,
                    "readstates" => CallFlags::READ_STATES,
                    "writestates" => CallFlags::WRITE_STATES,
                    "allowcall" => CallFlags::ALLOW_CALL,
                    "allownotify" => CallFlags::ALLOW_NOTIFY,
                    "states" => CallFlags::STATES,
                    "readonly" => CallFlags::from_bits(
                        (CallFlags::READ_STATES.0 | CallFlags::ALLOW_NOTIFY.0) as u32,
                    )
                    .unwrap_or(CallFlags::READ_STATES),
                    other => {
                        return Err(format!("Unsupported call flag value: {other}").into());
                    }
                };
                Ok(flag)
            }
            Value::Number(num) => {
                if let Some(value) = num.as_u64() {
                    if let Some(flags) = CallFlags::from_bits(value as u32) {
                        Ok(flags)
                    } else {
                        Err(format!("Unsupported call flag bits: {value}").into())
                    }
                } else {
                    Err("CallFlags numeric value must be unsigned".into())
                }
            }
            other => Err(format!("Unsupported CallFlags representation: {other}").into()),
        }
    }

    fn parse_invoke_arguments(value: &Value) -> Result<Vec<neo_vm::StackItem>> {
        let array = match value {
            Value::Array(items) => items,
            Value::Null => return Ok(Vec::new()),
            _ => return Err("Arguments must be an array".into()),
        };

        array
            .iter()
            .map(|item| Self::parse_invoke_argument(item))
            .collect()
    }

    fn parse_invoke_argument(value: &Value) -> Result<neo_vm::StackItem> {
        use base64::engine::general_purpose;
        use base64::Engine;
        use neo_core::{UInt160, UInt256};
        use neo_vm::StackItem;
        use std::collections::BTreeMap;

        let obj = value
            .as_object()
            .ok_or("Argument must be an object with type and value")?;

        let type_str = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Contract parameter missing type")?;

        let value_field = obj.get("value").unwrap_or(&Value::Null);

        match type_str {
            "Any" | "Null" => Ok(StackItem::Null),
            "Boolean" => {
                let flag = value_field
                    .as_bool()
                    .ok_or("Boolean parameter must be true or false")?;
                Ok(StackItem::Boolean(flag))
            }
            "Integer" => {
                let int_value = match value_field {
                    Value::Number(num) => num
                        .as_i64()
                        .ok_or("Integer parameter must be a signed 64-bit number")?,
                    Value::String(s) => s
                        .parse::<i64>()
                        .map_err(|_| "Integer parameter string must parse to i64")?,
                    _ => return Err("Integer parameter must be number or string".into()),
                };
                Ok(StackItem::from_int(int_value))
            }
            "ByteArray" => {
                let encoded = value_field
                    .as_str()
                    .ok_or("ByteArray parameter must be a string")?;
                let bytes = if let Some(stripped) = encoded.strip_prefix("0x") {
                    hex::decode(stripped).map_err(|_| "ByteArray hex decoding failed")?
                } else {
                    general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|_| "ByteArray base64 decoding failed")?
                };
                Ok(StackItem::from_byte_string(bytes))
            }
            "String" => {
                let s = value_field
                    .as_str()
                    .ok_or("String parameter must be a string value")?;
                Ok(StackItem::from_byte_string(s.as_bytes().to_vec()))
            }
            "Hash160" => {
                let s = value_field
                    .as_str()
                    .ok_or("Hash160 parameter must be a string")?;
                let hash = UInt160::from_str(s).map_err(|_| "Invalid Hash160 value")?;
                Ok(StackItem::from_byte_string(hash.as_bytes().to_vec()))
            }
            "Hash256" => {
                let s = value_field
                    .as_str()
                    .ok_or("Hash256 parameter must be a string")?;
                let hash = UInt256::from_str(s).map_err(|_| "Invalid Hash256 value")?;
                Ok(StackItem::from_byte_string(hash.as_bytes().to_vec()))
            }
            "PublicKey" | "Signature" => {
                let s = value_field
                    .as_str()
                    .ok_or("PublicKey/Signature parameter must be string")?;
                let bytes = if let Some(stripped) = s.strip_prefix("0x") {
                    hex::decode(stripped).map_err(|_| "Invalid hex encoding")?
                } else {
                    general_purpose::STANDARD
                        .decode(s)
                        .map_err(|_| "Invalid base64 encoding")?
                };
                Ok(StackItem::from_byte_string(bytes))
            }
            "Array" => {
                let array_value = value_field
                    .as_array()
                    .ok_or("Array parameter must be an array")?;
                let items = array_value
                    .iter()
                    .map(|elem| Self::parse_invoke_argument(elem))
                    .collect::<Result<Vec<_>>>()?;
                Ok(StackItem::from_array(items))
            }
            "Map" => {
                let entries = value_field
                    .as_array()
                    .ok_or("Map parameter must be an array of key/value pairs")?;
                let mut map = BTreeMap::new();
                for entry in entries {
                    let entry_obj = entry
                        .as_object()
                        .ok_or("Map entry must be an object with key and value")?;
                    let key_value = entry_obj.get("key").ok_or("Map entry missing key field")?;
                    let value_value = entry_obj
                        .get("value")
                        .ok_or("Map entry missing value field")?;
                    let key = Self::parse_invoke_argument(key_value)?;
                    let value = Self::parse_invoke_argument(value_value)?;
                    map.insert(key, value);
                }
                Ok(StackItem::from_map(map))
            }
            other => Err(format!("Unsupported contract parameter type: {other}").into()),
        }
    }

    pub async fn get_contract_state(&self, params: Value) -> Result<Value> {
        let params_array = params.as_array().ok_or("Invalid parameters format")?;
        if params_array.is_empty() {
            return Err("Missing contract identifier".into());
        }

        let hash = self.resolve_contract_hash(&params_array[0]).await?;
        if let Some(state) = self.ledger.get_contract_state(&hash).await? {
            self.contract_state_to_json(&state)
        } else {
            let fallback = self
                .ledger
                .list_native_contracts()
                .await
                .into_iter()
                .find(|c| c.hash == hash);
            match fallback {
                Some(state) => self.contract_state_to_json(&state),
                None => Ok(Value::Null),
            }
        }
    }

    pub async fn get_transaction_height(&self, params: Value) -> Result<Value> {
        let params_array = params.as_array().ok_or("Invalid parameters format")?;
        if params_array.is_empty() {
            return Err("Missing transaction hash".into());
        }

        let hash_str = params_array[0]
            .as_str()
            .ok_or("Invalid transaction hash format")?;
        let hash = Self::parse_uint256(hash_str)?;

        match self.ledger.get_transaction_height(&hash).await? {
            Some(height) => Ok(json!(height)),
            None => Ok(Value::Null),
        }
    }
}
