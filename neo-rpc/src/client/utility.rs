mod attributes;
mod nep;
mod parsing;
mod stack;
mod tx_json;
mod witness;
mod witness_rule;

#[allow(unused_imports)]
pub use attributes::attribute_from_json;
pub(crate) use nep::{
    NepBalanceFieldRefs, NepTransferFieldRefs, balance_list_to_json, insert_nep_balance_fields,
    insert_nep_transfer_fields, parse_balance_list, parse_nep_balance_fields,
    parse_nep_transfer_fields, parse_transfer_lists, transfer_lists_to_json,
};
pub use parsing::optional_string;
pub use parsing::JsonParseError;
pub(crate) use parsing::{base64_string_token, optional_base64_field_lossy};
#[allow(unused_imports)]
pub use parsing::{
    cloned_token_array, empty_array, insert_optional_string, jtoken_to_serde, object_array,
    object_array_from_iter, optional_script_hash_or_address_lossy, optional_string_or_null,
    parse_base64_token, parse_i64_token, parse_nonce_token, parse_number_or_string_token,
    parse_object_array_lossy, parse_optional_present_token_array_strict,
    parse_optional_string_array_strict, parse_optional_token_array_strict,
    parse_oracle_response_code, parse_script_hash_or_address, parse_string_array_lossy,
    parse_u32_token, parse_u64_token, parse_uint256_array_lossy, required_address_script_hash,
    required_bigint_string, required_script_hash_or_address, required_string, required_u16_number,
    required_u32_number, required_u64_number, required_uint256, token_array,
};
pub use stack::{stack_items_from_json_field, stack_items_to_json};
#[allow(unused_imports)]
pub use witness::{
    payload_witness_from_json, payload_witness_to_json, scripts_to_witness_json, witness_to_json,
};
#[allow(unused_imports)]
pub use witness_rule::rule_from_json;

use neo_config::ProtocolSettings;
use neo_crypto::{ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use neo_execution::Contract;
use neo_native_contracts::NativeRegistry;
use neo_payloads::{Block, BlockHeader, Transaction, Witness};
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::{JObject, JToken};
use neo_vm_rs::StackValue;
use neo_wallets::KeyPair;
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;
use num_bigint::BigInt;
use std::sync::OnceLock;

/// Utility functions for RPC client
/// Matches C# Utility class
pub struct RpcUtility;

impl RpcUtility {
    fn native_registry() -> &'static NativeRegistry {
        static REGISTRY: OnceLock<NativeRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            // `NativeRegistry::new()` is empty by design; populate it
            // with the canonical standard native-contract set.
            use neo_execution::native_contract_provider::NativeContractProvider;
            let mut registry = NativeRegistry::new();
            for contract in
                neo_native_contracts::StandardNativeProvider::new().all_native_contracts()
            {
                registry.register(contract);
            }
            registry
        })
    }

    /// Converts a `JToken` to a script hash
    /// Matches C# `ToScriptHash` extension
    pub fn to_script_hash(
        value: &JToken,
        protocol_settings: &ProtocolSettings,
    ) -> CoreResult<UInt160> {
        let address_or_script_hash = value
            .as_string()
            .ok_or_else(|| CoreError::other("Value is not a string"))?;

        if address_or_script_hash.len() < 40 && !address_or_script_hash.starts_with("0x") {
            WalletHelper::to_script_hash(
                &address_or_script_hash,
                protocol_settings.address_version,
            )
        } else {
            UInt160::parse(&address_or_script_hash).map_err(|e| CoreError::other(e.to_string()))
        }
    }

    /// Converts an address or script hash string to script hash string
    /// Matches C# `AsScriptHash` extension
    #[must_use]
    pub fn as_script_hash(address_or_script_hash: &str) -> String {
        for contract in Self::native_registry().contracts() {
            if address_or_script_hash.eq_ignore_ascii_case(contract.name())
                || address_or_script_hash == contract.id().to_string()
            {
                return contract.hash().to_string();
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

    /// Parse WIF or private key hex string to `KeyPair`
    /// Matches C# `GetKeyPair`
    pub fn key_pair(key: &str) -> CoreResult<KeyPair> {
        if key.is_empty() {
            return Err(CoreError::other("Key cannot be empty"));
        }

        let key = key.strip_prefix("0x").unwrap_or(key);

        match key.len() {
            52 => {
                // WIF format
                KeyPair::from_wif(key).map_err(|e| CoreError::other(e.to_string()))
            }
            64 => {
                // Hex private key
                let bytes =
                    hex::decode(key).map_err(|e| CoreError::other(e.to_string()))?;
                KeyPair::from_private_key(&bytes).map_err(|e| CoreError::other(e.to_string()))
            }
            _ => Err(CoreError::other("Invalid key format")),
        }
    }

    /// Parse address, scripthash or public key string to `UInt160`
    /// Matches C# `GetScriptHash`
    pub fn get_script_hash(
        account: &str,
        protocol_settings: &ProtocolSettings,
    ) -> CoreResult<UInt160> {
        if account.is_empty() {
            return Err(CoreError::other("Account cannot be empty"));
        }

        let account = account.strip_prefix("0x").unwrap_or(account);

        match account.len() {
            34 => {
                // Address
                WalletHelper::to_script_hash(account, protocol_settings.address_version)
            }
            40 => {
                // Script hash
                UInt160::parse(account).map_err(|e| CoreError::other(e.to_string()))
            }
            66 => {
                // Public key - Neo N3 uses secp256r1 (NIST P-256) curve
                let key_bytes = hex::decode(account)
                    .map_err(|err| CoreError::other(format!("Invalid public key hex: {err}")))?;
                let point =
                    ECPoint::decode_compressed_with_curve(ECCurve::Secp256r1, &key_bytes)
                        .map_err(|err| CoreError::other(err.to_string()))?;
                let script = Contract::create_signature_redeem_script(point);
                Ok(UInt160::from_script(&script))
            }
            _ => Err(CoreError::other("Invalid account format")),
        }
    }

    /// Converts a block to JSON representation.
    #[must_use]
    pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
        tx_json::block_to_json(block, protocol_settings)
    }

    /// Parses a block header from JSON
    pub fn header_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> CoreResult<BlockHeader> {
        let version = json
            .get("version")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'version' field"))? as u32;

        let previous_hash = json
            .get("previousblockhash")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|value| UInt256::parse(&value).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'previousblockhash' field"))?;

        let merkle_root = json
            .get("merkleroot")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|value| UInt256::parse(&value).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'merkleroot' field"))?;

        let timestamp = json
            .get("time")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'time' field"))? as u64;

        let nonce_token = json
            .get("nonce")
            .ok_or_else(|| CoreError::other("Missing 'nonce' field for header parsing"))?;
        let nonce = parse_nonce_token(nonce_token)?;

        let index = json
            .get("index")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'index' field"))? as u32;

        let primary_index = json
            .get("primary")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'primary' field"))? as u8;

        let next_consensus_text = json
            .get("nextconsensus")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'nextconsensus' field"))?;
        let next_consensus = Self::get_script_hash(&next_consensus_text, protocol_settings)
            .map_err(|err| {
                CoreError::other(format!(
                    "Invalid 'nextconsensus' field in block header: {err}"
                ))
            })?;

        let witnesses = parse_optional_token_array_strict(
            json,
            "witnesses",
            "Witness entry must be an object",
            |token| {
                let obj = token
                    .as_object()
                    .ok_or_else(|| CoreError::other("Witness entry must be an object"))?;
                Self::witness_from_json(obj)
            },
        )?;

        Ok(BlockHeader::new_with_witnesses(
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
    /// Matches C# `BlockFromJson`
    pub fn block_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> CoreResult<Block> {
        tx_json::block_from_json(json, protocol_settings, Self::header_from_json)
    }

    /// Converts a transaction to JSON
    /// Matches C# `TransactionToJson`
    pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
        tx_json::transaction_to_json(tx, protocol_settings)
    }

    /// Converts JSON to a transaction
    /// Matches C# `TransactionFromJson`
    pub fn transaction_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> CoreResult<Transaction> {
        tx_json::transaction_from_json(json, protocol_settings)
    }

    /// Converts a `neo-serialization::json` representation of a stack item into `neo-vm-rs`.
    pub fn stack_item_from_json(json: &JObject) -> CoreResult<StackValue> {
        stack::stack_item_from_json(json).map_err(|e| CoreError::other(e.to_string()))
    }

    /// Converts a `neo-vm-rs` stack value into a `neo-serialization::json` representation.
    pub fn stack_item_to_json(item: &StackValue) -> CoreResult<JObject> {
        stack::stack_item_to_json(item)
    }

    /// Converts an RPC stack value using the same integer rules as local VM clients.
    pub fn stack_value_to_bigint(value: &StackValue) -> CoreResult<BigInt> {
        stack::stack_value_to_bigint(value)
    }

    /// Converts an RPC stack value using NeoVM truthiness rules.
    #[must_use]
    pub fn stack_value_to_bool(value: &StackValue) -> bool {
        stack::stack_value_to_bool(value)
    }

    /// Converts an RPC stack value to a display/API string.
    pub fn stack_value_to_string(value: &StackValue) -> CoreResult<String> {
        stack::stack_value_to_string(value)
    }

    /// Creates a witness from JSON (invocation/verification scripts encoded as base64).
    pub fn witness_from_json(json: &JObject) -> CoreResult<Witness> {
        witness::witness_from_json(json)
    }

    /// Parses a witness rule from JSON (RPC utility parity).
    pub fn rule_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> CoreResult<neo_payloads::WitnessRule> {
        witness_rule::rule_from_json(json, protocol_settings)
    }
}

/// Public wrappers matching the historical `crate::utility::function` style.
pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
    RpcUtility::block_to_json(block, protocol_settings)
}

pub fn block_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> CoreResult<Block> {
    RpcUtility::block_from_json(json, protocol_settings)
}

pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
    RpcUtility::transaction_to_json(tx, protocol_settings)
}

pub fn transaction_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> CoreResult<Transaction> {
    RpcUtility::transaction_from_json(json, protocol_settings)
}

pub fn stack_item_from_json(json: &JObject) -> CoreResult<StackValue> {
    RpcUtility::stack_item_from_json(json)
}

pub fn stack_item_to_json(item: &StackValue) -> CoreResult<JObject> {
    RpcUtility::stack_item_to_json(item)
}

pub fn witness_from_json(json: &JObject) -> CoreResult<Witness> {
    RpcUtility::witness_from_json(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use neo_payloads::WitnessCondition;
    use neo_payloads::OracleResponseCode;
    use neo_payloads::{Signer, TransactionAttribute};
    use neo_primitives::{ADDRESS_SIZE, UInt256, WitnessScope};
    use neo_serialization::json::JArray;

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
    fn get_key_pair_accepts_wif_and_hex() {
        let wif = "KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p";
        let expected = KeyPair::from_wif(wif).expect("keypair");

        let parsed = RpcUtility::key_pair(wif).expect("wif parse");
        assert_eq!(parsed, expected);

        let hex_key = hex::encode(expected.private_key());
        let parsed = RpcUtility::key_pair(&hex_key).expect("hex parse");
        assert_eq!(parsed, expected);

        let hex_prefixed = format!("0x{hex_key}");
        let parsed = RpcUtility::key_pair(&hex_prefixed).expect("hex parse");
        assert_eq!(parsed, expected);
    }

    #[test]
    fn get_key_pair_rejects_invalid_input() {
        let err = RpcUtility::key_pair("").expect_err("empty");
        assert_eq!(err.to_string(), "Key cannot be empty");

        let err = RpcUtility::key_pair("00").expect_err("invalid");
        assert_eq!(err.to_string(), "Invalid key format");
    }

    #[test]
    fn get_script_hash_accepts_hash_and_rejects_invalid() {
        let hash = UInt160::zero();
        let hash_string = hex::encode(hash.to_array());

        let parsed =
            RpcUtility::get_script_hash(&hash_string, &ProtocolSettings::default_settings())
                .unwrap();
        assert_eq!(parsed, hash);

        let prefixed = format!("0x{hash_string}");
        let parsed =
            RpcUtility::get_script_hash(&prefixed, &ProtocolSettings::default_settings()).unwrap();
        assert_eq!(parsed, hash);

        let err = RpcUtility::get_script_hash("", &ProtocolSettings::default_settings())
            .expect_err("empty");
        assert_eq!(err.to_string(), "Account cannot be empty");

        let err = RpcUtility::get_script_hash("00", &ProtocolSettings::default_settings())
            .expect_err("invalid");
        assert_eq!(err.to_string(), "Invalid account format");
    }

    #[test]
    fn as_script_hash_maps_native_contract_name_and_id() {
        use neo_execution::native_contract_provider::NativeContractProvider;
        let contract = neo_native_contracts::StandardNativeProvider::new()
            .all_native_contracts()
            .into_iter()
            .find(|contract| contract.name() == "NeoToken")
            .expect("NeoToken contract");
        let expected_hash = contract.hash().to_string();
        let name = contract.name().to_string();
        let id = contract.id().to_string();

        assert_eq!(RpcUtility::as_script_hash(&name), expected_hash);
        assert_eq!(RpcUtility::as_script_hash(&id), expected_hash);
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
        assert!(matches!(item, StackValue::Boolean(true)));
    }

    #[test]
    fn stack_item_parses_interop_interface_without_value() {
        let mut obj = JObject::new();
        obj.insert(
            "type".to_string(),
            JToken::String("InteropInterface".to_string()),
        );
        obj.insert("id".to_string(), JToken::String("iter-1".to_string()));

        let item = RpcUtility::stack_item_from_json(&obj).expect("stack item");
        assert!(matches!(item, StackValue::Interop(0)));
    }

    #[test]
    fn stack_item_parses_bytestring_and_buffer() {
        let bytes = vec![1u8, 2, 3];
        let encoded = BASE64.encode(&bytes);

        let mut bytestring = JObject::new();
        bytestring.insert("type".to_string(), JToken::String("ByteString".to_string()));
        bytestring.insert("value".to_string(), JToken::String(encoded.clone()));
        let item = RpcUtility::stack_item_from_json(&bytestring).expect("bytestring");
        assert_eq!(item.as_bytes().expect("bytestring bytes"), bytes);

        let mut buffer = JObject::new();
        buffer.insert("type".to_string(), JToken::String("Buffer".to_string()));
        buffer.insert("value".to_string(), JToken::String(encoded));
        let item = RpcUtility::stack_item_from_json(&buffer).expect("buffer");
        assert_eq!(item.as_bytes().expect("buffer bytes"), bytes);
    }

    #[test]
    fn stack_item_parses_pointer_and_any() {
        let mut pointer = JObject::new();
        pointer.insert("type".to_string(), JToken::String("Pointer".to_string()));
        pointer.insert("value".to_string(), JToken::String("7".to_string()));

        let item = RpcUtility::stack_item_from_json(&pointer).expect("pointer");
        assert!(matches!(item, StackValue::Pointer(7)));

        let mut any = JObject::new();
        any.insert("type".to_string(), JToken::String("Any".to_string()));
        let item = RpcUtility::stack_item_from_json(&any).expect("any");
        assert!(matches!(item, StackValue::Null));
    }

    #[test]
    fn stack_item_parses_any_with_value() {
        let mut any = JObject::new();
        any.insert("type".to_string(), JToken::String("Any".to_string()));
        any.insert("value".to_string(), JToken::String("data".to_string()));
        let item = RpcUtility::stack_item_from_json(&any).expect("any");
        assert_eq!(item.as_bytes().expect("bytes"), b"data");
    }

    #[test]
    fn stack_item_fallbacks_for_unknown_type() {
        let mut unknown = JObject::new();
        unknown.insert("type".to_string(), JToken::String("Unknown".to_string()));
        unknown.insert("value".to_string(), JToken::String("hello".to_string()));

        let item = RpcUtility::stack_item_from_json(&unknown).expect("fallback");
        assert_eq!(item.as_bytes().expect("bytes"), b"hello");

        let mut empty = JObject::new();
        empty.insert("type".to_string(), JToken::String("Unknown".to_string()));
        let item = RpcUtility::stack_item_from_json(&empty).expect("fallback null");
        assert!(matches!(item, StackValue::Null));
    }

    #[test]
    fn transaction_roundtrip_json() {
        use neo_payloads::witness::Witness as PayloadWitness;
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
    fn transaction_from_json_rejects_empty_optional_array_entries() {
        for (field, expected) in [
            ("signers", "Signer entry must be an object"),
            ("attributes", "Transaction attribute must be an object"),
            ("witnesses", "Witness entry must be an object"),
        ] {
            let mut entries = JArray::new();
            entries.add(None);
            let mut json = JObject::new();
            json.insert(field.to_string(), JToken::Array(entries));

            let err =
                RpcUtility::transaction_from_json(&json, &ProtocolSettings::default_settings())
                    .expect_err("empty array slot should fail");
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn block_roundtrip_json() {
        let witness = Witness::new_with_scripts(vec![0xAA, 0xBB], vec![0xCC, 0xDD]);
        let header = BlockHeader::new_with_witnesses(
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
        let block = Block::from_parts(header, vec![tx]);

        let json = RpcUtility::block_to_json(&block, &ProtocolSettings::default_settings());
        let parsed =
            RpcUtility::block_from_json(&json, &ProtocolSettings::default_settings()).unwrap();

        assert_eq!(parsed.header.index(), block.header.index());
        assert_eq!(parsed.header.timestamp(), block.header.timestamp());
        assert_eq!(parsed.transactions.len(), 1);
        assert_eq!(parsed.transactions[0].script(), b"\t\t\t");
        // header carries exactly one witness by type (single `witness` field)
        assert_eq!(
            parsed.header.witness.invocation_script(),
            witness.invocation_script()
        );
    }

    #[test]
    fn header_from_json_rejects_empty_witness_entry() {
        let mut witnesses = JArray::new();
        witnesses.add(None);

        let mut json = JObject::new();
        json.insert("version".to_string(), JToken::Number(0.0));
        json.insert(
            "previousblockhash".to_string(),
            JToken::String(UInt256::zero().to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            JToken::String(UInt256::zero().to_string()),
        );
        json.insert("time".to_string(), JToken::Number(123.0));
        json.insert(
            "nonce".to_string(),
            JToken::String(format!("{:016X}", 42u64)),
        );
        json.insert("index".to_string(), JToken::Number(5.0));
        json.insert("primary".to_string(), JToken::Number(0.0));
        json.insert(
            "nextconsensus".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );
        json.insert("witnesses".to_string(), JToken::Array(witnesses));

        let err = RpcUtility::header_from_json(&json, &ProtocolSettings::default_settings())
            .expect_err("empty witness slot should fail");
        assert_eq!(err.to_string(), "Witness entry must be an object");
    }

    #[test]
    fn transaction_roundtrip_with_custom_signer() {
        use neo_payloads::WitnessRuleAction;

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
        signer.rules.push(neo_payloads::WitnessRule::new(
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
        assert!(
            parsed_signer
                .scopes
                .contains(WitnessScope::CUSTOM_CONTRACTS)
        );
        assert_eq!(parsed_signer.allowed_contracts, signer.allowed_contracts);
        assert_eq!(parsed_signer.allowed_groups.len(), 1);
        assert_eq!(parsed_signer.rules.len(), 1);
    }

    #[test]
    fn transaction_roundtrip_preserves_attributes() {
        use neo_payloads::OracleResponseCode;
        use neo_payloads::conflicts::Conflicts;
        use neo_payloads::not_valid_before::NotValidBefore;
        use neo_payloads::notary_assisted::NotaryAssisted;
        use neo_payloads::oracle_response::OracleResponse;

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
        let StackValue::Array(array_items) = item_array else {
            panic!("expected array");
        };
        assert_eq!(array_items.len(), 1);

        let mut struct_obj = JObject::new();
        struct_obj.insert("type".to_string(), JToken::String("Struct".to_string()));
        struct_obj.insert("value".to_string(), JToken::Array(array));
        let item_struct = RpcUtility::stack_item_from_json(&struct_obj).unwrap();
        let StackValue::Struct(struct_items) = item_struct else {
            panic!("expected struct");
        };
        assert_eq!(struct_items.len(), 1);
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
        let StackValue::Map(map) = item_map else {
            panic!("expected map");
        };
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn stack_item_to_json_emits_array_and_map_shapes() {
        let array = StackValue::Array(vec![StackValue::Integer(5)]);
        assert_eq!(
            RpcUtility::stack_item_to_json(&array).unwrap().to_string(),
            r#"{"type":"Array","value":[{"type":"Integer","value":"5"}]}"#
        );

        let map = StackValue::Map(vec![(
            StackValue::ByteString(b"k".to_vec()),
            StackValue::Boolean(true),
        )]);
        let expected_map = concat!(
            r#"{"type":"Map","value":[{"key":{"type":"ByteString","value":"aw=="},"#,
            r#""value":{"type":"Boolean","value":true}}]}"#,
        );
        assert_eq!(
            RpcUtility::stack_item_to_json(&map).unwrap().to_string(),
            expected_map
        );
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
