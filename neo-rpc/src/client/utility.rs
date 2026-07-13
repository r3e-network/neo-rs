mod attributes;
mod nep;
mod parsing;
mod stack;
mod tx_json;
mod witness;
mod witness_rule;

// Rationale: these legacy public re-exports keep the client utility facade
// stable while internal modules own the actual parsing implementations.
#[allow(unused_imports)]
pub use attributes::attribute_from_json;
pub(crate) use nep::{
    NepBalanceFieldRefs, NepTransferFieldRefs, balance_list_to_json, insert_nep_balance_fields,
    insert_nep_transfer_fields, parse_balance_list, parse_nep_balance_fields,
    parse_nep_transfer_fields, parse_transfer_lists, transfer_lists_to_json,
};
pub use parsing::JsonParseError;
pub use parsing::optional_string;
#[cfg(feature = "server")]
pub(crate) use parsing::parse_script_hash_or_address_inner;
pub(crate) use parsing::{base64_string_token, optional_base64_field_lossy};
// Rationale: these helpers are intentionally re-exported as the client JSON
// compatibility toolkit; individual builds may not use every helper.
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
// Rationale: witness JSON helpers remain part of the public compatibility
// facade even when a given feature set does not call all of them.
#[allow(unused_imports)]
pub use witness::{
    payload_witness_from_json, payload_witness_to_json, scripts_to_witness_json, witness_to_json,
};
// Rationale: witness-rule parsing is exported for C# wallet/RPC JSON
// compatibility and can be unused in minimal client builds.
#[allow(unused_imports)]
pub use witness_rule::rule_from_json;

use neo_config::ProtocolSettings;
use neo_crypto::{ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use neo_execution::Contract;
use neo_native_contracts::{NativeRegistry, StandardNativeProvider};
use neo_payloads::{Block, BlockHeader, Transaction, Witness};
use neo_primitives::{UInt160, UInt256, strip_hex_prefix};
use neo_serialization::json::{JObject, JToken};
use neo_vm::StackValue;
use neo_wallets::KeyPair;
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;
use num_bigint::BigInt;
use std::sync::OnceLock;

/// Utility functions for RPC client
/// Matches C# Utility class
pub struct RpcUtility;

impl RpcUtility {
    fn native_registry() -> &'static NativeRegistry<StandardNativeProvider> {
        static REGISTRY: OnceLock<NativeRegistry<StandardNativeProvider>> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            // `NativeRegistry::new()` is empty by design; populate it
            // with the canonical standard native-contract set.
            let mut registry = NativeRegistry::<StandardNativeProvider>::new();
            for contract in neo_native_contracts::standard_native_contracts() {
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

        let has_hex_prefix =
            strip_hex_prefix(&address_or_script_hash) != address_or_script_hash.as_str();
        if address_or_script_hash.len() < 40 && !has_hex_prefix {
            WalletHelper::to_script_hash(&address_or_script_hash, protocol_settings.address_version)
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

        let key = strip_hex_prefix(key);

        match key.len() {
            52 => {
                // WIF format
                KeyPair::from_wif(key).map_err(|e| CoreError::other(e.to_string()))
            }
            64 => {
                // Hex private key
                let bytes = hex::decode(key).map_err(|e| CoreError::other(e.to_string()))?;
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

        let account = strip_hex_prefix(account);

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
                let point = ECPoint::decode_compressed_with_curve(ECCurve::Secp256r1, &key_bytes)
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
            .ok_or_else(|| CoreError::other("Missing or invalid 'version' field"))?
            as u32;

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
            .ok_or_else(|| CoreError::other("Missing or invalid 'time' field"))?
            as u64;

        let nonce_token = json
            .get("nonce")
            .ok_or_else(|| CoreError::other("Missing 'nonce' field for header parsing"))?;
        let nonce = parse_nonce_token(nonce_token)?;

        let index = json
            .get("index")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'index' field"))?
            as u32;

        let primary_index = json
            .get("primary")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'primary' field"))?
            as u8;

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

    /// Converts a `neo-serialization::json` representation of a stack item into `neo-vm`.
    pub fn stack_item_from_json(json: &JObject) -> CoreResult<StackValue> {
        stack::stack_item_from_json(json).map_err(|e| CoreError::other(e.to_string()))
    }

    /// Converts a `neo-vm` stack value into a `neo-serialization::json` representation.
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

pub fn block_from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Block> {
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
#[path = "../tests/client/utility.rs"]
mod tests;
