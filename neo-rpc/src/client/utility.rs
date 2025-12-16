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

mod attributes;
mod parsing;
mod stack;
mod tx_json;
mod witness;

#[allow(unused_imports)]
pub use attributes::attribute_from_json;
#[allow(unused_imports)]
pub use parsing::{
    jobject_to_serde, jtoken_to_serde, parse_base64_token, parse_i64_token, parse_nonce_token,
    parse_oracle_response_code, parse_u32_token, parse_u64_token,
};
#[allow(unused_imports)]
pub use witness::{
    payload_witness_from_json, payload_witness_to_json, scripts_to_witness_json, witness_to_json,
};

use neo_config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::{Block, BlockHeader, Contract, ECCurve, ECPoint, KeyPair, Transaction, Witness};
use neo_json::{JObject, JToken};
use neo_primitives::{UInt160, UInt256};
use neo_vm::StackItem;

/// Utility functions for RPC client
/// Matches C# Utility class
pub struct RpcUtility;

impl RpcUtility {
    /// Converts a JToken to a script hash
    /// Matches C# ToScriptHash extension
    pub fn to_script_hash(
        value: &JToken,
        protocol_settings: &ProtocolSettings,
    ) -> Result<UInt160, String> {
        let address_or_script_hash = value.as_string().ok_or("Value is not a string")?;

        if address_or_script_hash.len() < 40 && !address_or_script_hash.starts_with("0x") {
            WalletHelper::to_script_hash(&address_or_script_hash, protocol_settings.address_version)
                .map_err(|e| e.to_string())
        } else {
            UInt160::parse(&address_or_script_hash).map_err(|e| e.to_string())
        }
    }

    /// Converts an address or script hash string to script hash string
    /// Matches C# AsScriptHash extension
    pub fn as_script_hash(address_or_script_hash: &str) -> String {
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

        let key = key.strip_prefix("0x").unwrap_or(key);

        match key.len() {
            52 => {
                // WIF format
                KeyPair::from_wif(key).map_err(|e| e.to_string())
            }
            64 => {
                // Hex private key
                let bytes = hex::decode(key).map_err(|e| e.to_string())?;
                KeyPair::from_private_key(&bytes).map_err(|e| e.to_string())
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

        let account = account.strip_prefix("0x").unwrap_or(account);

        match account.len() {
            34 => {
                // Address
                WalletHelper::to_script_hash(account, protocol_settings.address_version)
                    .map_err(|e| e.to_string())
            }
            40 => {
                // Script hash
                UInt160::parse(account).map_err(|e| e.to_string())
            }
            66 => {
                // Public key - Neo N3 uses secp256r1 (NIST P-256) curve
                let key_bytes =
                    hex::decode(account).map_err(|err| format!("Invalid public key hex: {err}"))?;
                let point = ECPoint::decode_compressed_with_curve(ECCurve::Secp256r1, &key_bytes)
                    .map_err(|err| err.to_string())?;
                let script = Contract::create_signature_redeem_script(point);
                Ok(UInt160::from_script(&script))
            }
            _ => Err("Invalid account format".to_string()),
        }
    }

    /// Converts a block to JSON representation.
    pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
        tx_json::block_to_json(block, protocol_settings)
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
        let next_consensus = Self::get_script_hash(&next_consensus_text, protocol_settings)
            .map_err(|err| format!("Invalid 'nextconsensus' field in block header: {err}"))?;

        let witnesses = json
            .get("witnesses")
            .and_then(|token| token.as_array())
            .map(|entries| {
                entries
                    .children()
                    .iter()
                    .map(|entry| {
                        let obj = entry
                            .as_ref()
                            .and_then(|token| token.as_object())
                            .ok_or_else(|| "Witness entry must be an object".to_string())?;
                        Self::witness_from_json(obj)
                    })
                    .collect()
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
        tx_json::block_from_json(json, protocol_settings, Self::header_from_json)
    }

    /// Converts a transaction to JSON
    /// Matches C# TransactionToJson
    pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
        tx_json::transaction_to_json(tx, protocol_settings)
    }

    /// Converts JSON to a transaction
    /// Matches C# TransactionFromJson
    pub fn transaction_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Transaction, String> {
        tx_json::transaction_from_json(json, protocol_settings)
    }

    /// Converts a `neo-json` representation of a stack item back into a VM stack item.
    pub fn stack_item_from_json(json: &JObject) -> Result<StackItem, String> {
        stack::stack_item_from_json(json)
    }

    /// Creates a witness from JSON (invocation/verification scripts encoded as base64).
    pub fn witness_from_json(json: &JObject) -> Result<Witness, String> {
        witness::witness_from_json(json)
    }
}

/// Public wrappers matching the historical `crate::utility::function` style.
pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
    RpcUtility::block_to_json(block, protocol_settings)
}

pub fn block_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> Result<Block, String> {
    RpcUtility::block_from_json(json, protocol_settings)
}

pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
    RpcUtility::transaction_to_json(tx, protocol_settings)
}

pub fn transaction_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> Result<Transaction, String> {
    RpcUtility::transaction_from_json(json, protocol_settings)
}

pub fn stack_item_from_json(json: &JObject) -> Result<StackItem, String> {
    RpcUtility::stack_item_from_json(json)
}

pub fn witness_from_json(json: &JObject) -> Result<Witness, String> {
    RpcUtility::witness_from_json(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine as _;
    use neo_core::network::p2p::payloads::oracle_response_code::OracleResponseCode;
    use neo_core::{Signer, TransactionAttribute, WitnessCondition};
    use neo_json::JArray;
    use neo_primitives::{UInt256, WitnessScope, ADDRESS_SIZE};

    #[test]
    fn to_script_hash_accepts_address() {
        let hash = UInt160::zero();
        let address = hash.to_address();
        let token = JToken::String(address.clone());
        let parsed =
            RpcUtility::to_script_hash(&token, &ProtocolSettings::default_settings()).unwrap();
        assert_eq!(parsed, hash);
        assert_eq!(
            RpcUtility::get_script_hash(&hash.to_string(), &ProtocolSettings::default_settings())
                .unwrap(),
            hash
        );
    }

    #[test]
    fn get_script_hash_accepts_public_key_hex() {
        let keypair = KeyPair::generate().expect("keypair");
        let pubkey_hex = hex::encode(keypair.compressed_public_key());
        let expected_point = ECPoint::decode_compressed_with_curve(
            ECCurve::Secp256r1,
            &hex::decode(&pubkey_hex).unwrap(),
        )
        .unwrap();
        let expected_script = Contract::create_signature_redeem_script(expected_point);
        let expected_hash = UInt160::from_script(&expected_script);

        let parsed =
            RpcUtility::get_script_hash(&pubkey_hex, &ProtocolSettings::default_settings())
                .unwrap();
        assert_eq!(parsed, expected_hash);
    }

    #[test]
    fn witness_roundtrip_from_json() {
        let invocation = vec![1u8; ADDRESS_SIZE];
        let verification = vec![2u8; ADDRESS_SIZE];
        let mut obj = JObject::new();
        obj.insert(
            "invocation".to_string(),
            JToken::String(BASE64.encode(&invocation)),
        );
        obj.insert(
            "verification".to_string(),
            JToken::String(BASE64.encode(&verification)),
        );

        let witness = RpcUtility::witness_from_json(&obj).expect("witness");
        assert_eq!(witness.invocation_script(), invocation);
        assert_eq!(witness.verification_script(), verification);
    }

    #[test]
    fn stack_item_parses_boolean() {
        let mut obj = JObject::new();
        obj.insert("type".to_string(), JToken::String("Boolean".to_string()));
        obj.insert("value".to_string(), JToken::Boolean(true));

        let item = RpcUtility::stack_item_from_json(&obj).expect("stack item");
        assert!(item.as_bool().unwrap());
    }

    #[test]
    fn transaction_roundtrip_json() {
        use neo_core::network::p2p::payloads::witness::Witness as PayloadWitness;
        use neo_primitives::WitnessScope;

        let mut tx = Transaction::new();
        tx.set_version(1);
        tx.set_nonce(42);
        tx.set_script(vec![1, 2, 3, 4]);
        tx.set_system_fee(10);
        tx.set_network_fee(5);
        tx.set_valid_until_block(100);

        let signer = Signer::new(UInt160::zero(), WitnessScope::GLOBAL);
        tx.set_signers(vec![signer.clone()]);

        let witness = PayloadWitness::new_with_scripts(vec![1, 2], vec![3, 4]);
        tx.set_witnesses(vec![witness]);

        let json = RpcUtility::transaction_to_json(&tx, &ProtocolSettings::default_settings());
        let parsed =
            RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
                .expect("parse transaction");

        assert_eq!(parsed.version(), tx.version());
        assert_eq!(parsed.nonce(), tx.nonce());
        assert_eq!(parsed.system_fee(), tx.system_fee());
        assert_eq!(parsed.network_fee(), tx.network_fee());
        assert_eq!(parsed.valid_until_block(), tx.valid_until_block());
        assert_eq!(parsed.script(), tx.script());
        assert_eq!(parsed.signers().len(), 1);
        assert_eq!(parsed.signers()[0].account, signer.account);
        assert_eq!(parsed.witnesses().len(), 1);
    }

    #[test]
    fn block_roundtrip_json() {
        let witness = Witness::new_with_scripts(vec![0xAA, 0xBB], vec![0xCC, 0xDD]);
        let header = BlockHeader::new(
            0,
            UInt256::zero(),
            UInt256::zero(),
            12345,
            999,
            7,
            2,
            UInt160::zero(),
            vec![witness.clone()],
        );
        let mut tx = Transaction::new();
        tx.set_script(vec![9, 9, 9]);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        let block = Block::new(header, vec![tx]);

        let json = RpcUtility::block_to_json(&block, &ProtocolSettings::default_settings());
        let parsed =
            RpcUtility::block_from_json(&json, &ProtocolSettings::default_settings()).unwrap();

        assert_eq!(parsed.header.index, block.header.index);
        assert_eq!(parsed.header.timestamp, block.header.timestamp);
        assert_eq!(parsed.transactions.len(), 1);
        assert_eq!(parsed.transactions[0].script(), b"\t\t\t");
        assert_eq!(parsed.header.witnesses.len(), 1);
        assert_eq!(
            parsed.header.witnesses[0].invocation_script(),
            witness.invocation_script()
        );
    }

    #[test]
    fn transaction_roundtrip_with_custom_signer() {
        use neo_core::witness_rule::WitnessRuleAction;

        let mut tx = Transaction::new();
        tx.set_nonce(999);
        tx.set_script(vec![7, 7, 7]);

        let mut signer = Signer::new(UInt160::zero(), WitnessScope::CUSTOM_CONTRACTS);
        signer
            .allowed_contracts
            .push(UInt160::parse("0102030405060708090a0b0c0d0e0f1011121314").unwrap());
        signer.scopes |= WitnessScope::CUSTOM_GROUPS | WitnessScope::WITNESS_RULES;

        let key = KeyPair::generate().unwrap();
        let group_point = ECPoint::from_bytes(&key.compressed_public_key()).unwrap();
        signer.allowed_groups.push(group_point);
        signer.rules.push(neo_core::WitnessRule::new(
            WitnessRuleAction::Deny,
            WitnessCondition::CalledByEntry,
        ));

        tx.set_signers(vec![signer.clone()]);

        let json = RpcUtility::transaction_to_json(&tx, &ProtocolSettings::default_settings());
        let parsed =
            RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
                .expect("parse transaction");

        assert_eq!(parsed.signers().len(), 1);
        let parsed_signer = &parsed.signers()[0];
        assert_eq!(parsed_signer.account, signer.account);
        assert!(parsed_signer
            .scopes
            .contains(WitnessScope::CUSTOM_CONTRACTS));
        assert_eq!(parsed_signer.allowed_contracts, signer.allowed_contracts);
        assert_eq!(parsed_signer.allowed_groups.len(), 1);
        assert_eq!(parsed_signer.rules.len(), 1);
    }

    #[test]
    fn transaction_roundtrip_preserves_attributes() {
        use neo_core::network::p2p::payloads::conflicts::Conflicts;
        use neo_core::network::p2p::payloads::not_valid_before::NotValidBefore;
        use neo_core::network::p2p::payloads::notary_assisted::NotaryAssisted;
        use neo_core::network::p2p::payloads::oracle_response::OracleResponse;
        use neo_core::network::p2p::payloads::oracle_response_code::OracleResponseCode;

        let mut tx = Transaction::new();
        tx.set_nonce(1);
        tx.set_script(vec![1]);

        let attributes = vec![
            TransactionAttribute::HighPriority,
            TransactionAttribute::NotValidBefore(NotValidBefore::new(42)),
            TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())),
            TransactionAttribute::NotaryAssisted(NotaryAssisted::new(3)),
            TransactionAttribute::OracleResponse(OracleResponse::new(
                7,
                OracleResponseCode::Timeout,
                b"result".to_vec(),
            )),
        ];
        tx.set_attributes(attributes.clone());

        let json = RpcUtility::transaction_to_json(&tx, &ProtocolSettings::default_settings());
        let parsed =
            RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
                .expect("parse transaction");

        assert_eq!(parsed.attributes().len(), attributes.len());
        // Check representative attributes for correctness
        assert!(matches!(
            parsed.attributes()[0],
            TransactionAttribute::HighPriority
        ));
        if let TransactionAttribute::NotValidBefore(nvb) = &parsed.attributes()[1] {
            assert_eq!(nvb.height, 42);
        } else {
            panic!("expected NotValidBefore");
        }
        if let TransactionAttribute::Conflicts(conflicts) = &parsed.attributes()[2] {
            assert_eq!(conflicts.hash, UInt256::zero());
        } else {
            panic!("expected Conflicts");
        }
        if let TransactionAttribute::NotaryAssisted(notary) = &parsed.attributes()[3] {
            assert_eq!(notary.nkeys, 3);
        } else {
            panic!("expected NotaryAssisted");
        }
        if let TransactionAttribute::OracleResponse(resp) = &parsed.attributes()[4] {
            assert_eq!(resp.id, 7);
            assert_eq!(resp.code, OracleResponseCode::Timeout);
            assert_eq!(resp.result, b"result");
        } else {
            panic!("expected OracleResponse");
        }
    }

    #[test]
    fn parses_core_attributes_from_json() {
        let conflicts_hash = UInt256::zero().to_string();
        let conflicts_json = {
            let mut obj = JObject::new();
            obj.insert("type".to_string(), JToken::String("Conflicts".to_string()));
            obj.insert("hash".to_string(), JToken::String(conflicts_hash));
            obj
        };
        let conflicts = attribute_from_json(&conflicts_json).unwrap();
        assert!(matches!(conflicts, TransactionAttribute::Conflicts(_)));

        let mut nvb = JObject::new();
        nvb.insert(
            "type".to_string(),
            JToken::String("NotValidBefore".to_string()),
        );
        nvb.insert("height".to_string(), JToken::Number(5f64));
        let nvb_attr = attribute_from_json(&nvb).unwrap();
        assert!(matches!(nvb_attr, TransactionAttribute::NotValidBefore(_)));

        let mut notary = JObject::new();
        notary.insert(
            "type".to_string(),
            JToken::String("NotaryAssisted".to_string()),
        );
        notary.insert("nkeys".to_string(), JToken::Number(2f64));
        let notary_attr = attribute_from_json(&notary).unwrap();
        assert!(matches!(
            notary_attr,
            TransactionAttribute::NotaryAssisted(_)
        ));

        let mut oracle = JObject::new();
        oracle.insert(
            "type".to_string(),
            JToken::String("OracleResponse".to_string()),
        );
        oracle.insert("id".to_string(), JToken::Number(1f64));
        oracle.insert("code".to_string(), JToken::String("Timeout".to_string()));
        oracle.insert(
            "result".to_string(),
            JToken::String(BASE64.encode(b"hello")),
        );
        let oracle_attr = attribute_from_json(&oracle).unwrap();
        assert!(matches!(
            oracle_attr,
            TransactionAttribute::OracleResponse(_)
        ));
    }

    #[test]
    fn stack_item_parses_array_and_struct() {
        let mut child = JObject::new();
        child.insert("type".to_string(), JToken::String("Integer".to_string()));
        child.insert("value".to_string(), JToken::String("5".to_string()));

        let array = JArray::from(vec![JToken::Object(child.clone())]);
        let mut array_obj = JObject::new();
        array_obj.insert("type".to_string(), JToken::String("Array".to_string()));
        array_obj.insert("value".to_string(), JToken::Array(array.clone()));

        let item_array = RpcUtility::stack_item_from_json(&array_obj).unwrap();
        assert_eq!(item_array.as_array().unwrap().len(), 1);

        let mut struct_obj = JObject::new();
        struct_obj.insert("type".to_string(), JToken::String("Struct".to_string()));
        struct_obj.insert("value".to_string(), JToken::Array(array));
        let item_struct = RpcUtility::stack_item_from_json(&struct_obj).unwrap();
        assert_eq!(item_struct.as_array().unwrap().len(), 1);
    }

    #[test]
    fn stack_item_parses_map() {
        let mut key = JObject::new();
        key.insert("type".to_string(), JToken::String("ByteString".to_string()));
        key.insert(
            "value".to_string(),
            JToken::String(BASE64.encode("k".as_bytes())),
        );

        let mut value = JObject::new();
        value.insert("type".to_string(), JToken::String("Integer".to_string()));
        value.insert("value".to_string(), JToken::String("2".to_string()));

        let mut entry = JObject::new();
        entry.insert("key".to_string(), JToken::Object(key));
        entry.insert("value".to_string(), JToken::Object(value));

        let map_array = JArray::from(vec![JToken::Object(entry)]);
        let mut map_obj = JObject::new();
        map_obj.insert("type".to_string(), JToken::String("Map".to_string()));
        map_obj.insert("value".to_string(), JToken::Array(map_array));

        let item_map = RpcUtility::stack_item_from_json(&map_obj).unwrap();
        #[allow(clippy::mutable_key_type)]
        let map = item_map.as_map().unwrap();
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn parses_oracle_response_code_variants() {
        let string_code =
            parse_oracle_response_code(&JToken::String("ConsensusUnreachable".to_string()))
                .unwrap();
        assert_eq!(string_code, OracleResponseCode::ConsensusUnreachable);

        let hex_code = parse_oracle_response_code(&JToken::String("0x1f".to_string())).unwrap();
        assert_eq!(hex_code, OracleResponseCode::ContentTypeNotSupported);

        let numeric = parse_oracle_response_code(&JToken::Number(0xff as f64)).unwrap();
        assert_eq!(numeric, OracleResponseCode::Error);

        let invalid = parse_oracle_response_code(&JToken::String("UnknownCode".to_string()));
        assert!(invalid.is_err());

        // round-trip string mapping
        assert_eq!(
            super::parsing::oracle_response_code_to_str(OracleResponseCode::Timeout),
            "Timeout"
        );
    }
}
