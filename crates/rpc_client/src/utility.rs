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

use neo_core::{
    Block, BlockHeader, Contract, ECPoint, KeyPair, NativeContract, ProtocolSettings, Transaction,
    UInt160, UInt256, Wallet, Witness,
};
use neo_json::{JObject, JToken};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

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

    /// Converts a block to JSON
    /// Matches C# BlockToJson
    pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();

        // Add block header fields
        json.insert("hash".to_string(), JToken::String(block.hash().to_string()));
        json.insert("size".to_string(), JToken::Number(block.size() as f64));
        json.insert("version".to_string(), JToken::Number(block.version as f64));
        json.insert(
            "previousblockhash".to_string(),
            JToken::String(block.prev_hash.to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            JToken::String(block.merkle_root.to_string()),
        );
        json.insert("time".to_string(), JToken::Number(block.timestamp as f64));
        json.insert(
            "nonce".to_string(),
            JToken::String(format!("{:016x}", block.nonce)),
        );
        json.insert("index".to_string(), JToken::Number(block.index as f64));
        json.insert(
            "primary".to_string(),
            JToken::Number(block.primary_index as f64),
        );

        if let Some(ref next_consensus) = block.next_consensus {
            json.insert(
                "nextconsensus".to_string(),
                JToken::String(next_consensus.to_address(protocol_settings.address_version)),
            );
        }

        // Add witness
        json.insert(
            "witnesses".to_string(),
            JToken::Array(
                block
                    .witness
                    .iter()
                    .map(|w| JToken::Object(witness_to_json(w)))
                    .collect(),
            ),
        );

        // Add transactions
        json.insert(
            "tx".to_string(),
            JToken::Array(
                block
                    .transactions
                    .iter()
                    .map(|tx| JToken::Object(transaction_to_json(tx, protocol_settings)))
                    .collect(),
            ),
        );

        json
    }

    /// Converts JSON to a block
    /// Matches C# BlockFromJson
    pub fn block_from_json(
        json: &JObject,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Block, String> {
        // TODO: Implement block deserialization from JSON
        // This requires full Block implementation
        Err("Block deserialization not yet implemented".to_string())
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
            JToken::String(base64::encode(&tx.script())),
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
        // TODO: Implement transaction deserialization from JSON
        // This requires full Transaction implementation
        Err("Transaction deserialization not yet implemented".to_string())
    }
}

// Helper functions for JSON conversion

fn witness_to_json(witness: &neo_core::Witness) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "invocation".to_string(),
        JToken::String(base64::encode(&witness.invocation_script)),
    );
    json.insert(
        "verification".to_string(),
        JToken::String(base64::encode(&witness.verification_script)),
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

fn condition_to_json(condition: &neo_core::WitnessCondition) -> JObject {
    // TODO: Implement witness condition to JSON conversion
    JObject::new()
}

/// Creates a witness from JSON (invocation/verification scripts encoded as base64).
pub fn witness_from_json(json: &JObject) -> Result<Witness, String> {
    let invocation = json
        .get("invocation")
        .and_then(|v| v.as_string())
        .ok_or("Missing 'invocation' field")?;
    let verification = json
        .get("verification")
        .and_then(|v| v.as_string())
        .ok_or("Missing 'verification' field")?;

    let invocation_bytes =
        base64::decode(invocation).map_err(|err| format!("Invalid invocation script: {err}"))?;
    let verification_bytes = base64::decode(verification)
        .map_err(|err| format!("Invalid verification script: {err}"))?;

    Ok(Witness::new_with_scripts(
        invocation_bytes,
        verification_bytes,
    ))
}
