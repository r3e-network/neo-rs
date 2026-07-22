use base64::{Engine as _, engine::general_purpose};
use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_io::serializable::Serializable;
use neo_io::serializable::helper::SerializeHelper;
use neo_payloads::{Block, BlockHeader, Signer, Transaction};
use neo_serialization::json::{JObject, JToken};
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;

use super::attributes::attribute_from_json;
use super::parsing::{
    jtoken_to_serde, oracle_response_code_to_str, parse_i64_token,
    parse_optional_token_array_strict, parse_u32_token,
};
use super::witness::{payload_witness_from_json, payload_witness_to_json, witness_to_json};
use crate::types::json::{object_array, token_array};

/// Converts a block to JSON representation.
pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
    let mut json = JObject::new();
    let header = &block.header;

    json.insert("hash".to_string(), JToken::String(block.hash().to_string()));
    let block_size = header.size()
        + SerializeHelper::get_var_size(block.transactions.len() as u64)
        + block
            .transactions
            .iter()
            .map(neo_io::Serializable::size)
            .sum::<usize>();
    json.insert("size".to_string(), JToken::Number(block_size as f64));
    json.insert(
        "version".to_string(),
        JToken::Number(f64::from(header.version())),
    );
    json.insert(
        "previousblockhash".to_string(),
        JToken::String(header.prev_hash().to_string()),
    );
    json.insert(
        "merkleroot".to_string(),
        JToken::String(header.merkle_root().to_string()),
    );
    json.insert(
        "time".to_string(),
        JToken::Number(header.timestamp() as f64),
    );
    json.insert(
        "nonce".to_string(),
        JToken::String(format!("{:016X}", header.nonce())),
    );
    json.insert(
        "index".to_string(),
        JToken::Number(f64::from(header.index())),
    );
    json.insert(
        "primary".to_string(),
        JToken::Number(f64::from(header.primary_index())),
    );
    json.insert(
        "nextconsensus".to_string(),
        JToken::String(WalletHelper::to_address(
            header.next_consensus(),
            protocol_settings.address_version,
        )),
    );
    json.insert(
        "witnesses".to_string(),
        object_array(std::slice::from_ref(&header.witness), witness_to_json),
    );
    json.insert(
        "tx".to_string(),
        object_array(&block.transactions, |tx| {
            transaction_to_json(tx, protocol_settings)
        }),
    );

    json
}

/// Converts JSON to a block
/// Matches C# `BlockFromJson`
pub fn block_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
    header_parser: fn(&JObject, &ProtocolSettings) -> CoreResult<BlockHeader>,
) -> CoreResult<Block> {
    let header = header_parser(json, protocol_settings)?;

    let transactions = parse_optional_token_array_strict(
        json,
        "tx",
        "Transaction entry must be an object",
        |token| {
            let obj = token
                .as_object()
                .ok_or_else(|| CoreError::other("Transaction entry must be an object"))?;
            transaction_from_json(obj, protocol_settings)
        },
    )?;

    Ok(Block::from_parts(header, transactions))
}

/// Converts a transaction to JSON
/// Matches C# `TransactionToJson`
pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
    let mut json = JObject::new();

    json.insert("hash".to_string(), JToken::String(tx.hash().to_string()));
    json.insert("size".to_string(), JToken::Number(tx.size() as f64));
    json.insert(
        "version".to_string(),
        JToken::Number(f64::from(tx.version())),
    );
    json.insert("nonce".to_string(), JToken::Number(f64::from(tx.nonce())));
    json.insert(
        "sender".to_string(),
        tx.sender().map_or(JToken::Null, |sender| {
            JToken::String(WalletHelper::to_address(
                &sender,
                protocol_settings.address_version,
            ))
        }),
    );
    json.insert(
        "sysfee".to_string(),
        JToken::String(tx.system_fee().to_string()),
    );
    json.insert(
        "netfee".to_string(),
        JToken::String(tx.network_fee().to_string()),
    );
    json.insert(
        "validuntilblock".to_string(),
        JToken::Number(f64::from(tx.valid_until_block())),
    );

    // Add signers
    json.insert(
        "signers".to_string(),
        object_array(tx.signers(), |signer| {
            signer_to_json(signer, protocol_settings)
        }),
    );

    // Add attributes
    json.insert(
        "attributes".to_string(),
        object_array(tx.attributes(), attribute_to_json),
    );

    // Add script
    json.insert(
        "script".to_string(),
        JToken::String(general_purpose::STANDARD.encode(tx.script())),
    );

    // Add witnesses
    json.insert(
        "witnesses".to_string(),
        object_array(tx.witnesses(), payload_witness_to_json),
    );

    json
}

/// Converts JSON to a transaction
/// Matches C# `TransactionFromJson`
pub fn transaction_from_json(
    json: &JObject,
    _protocol_settings: &ProtocolSettings,
) -> CoreResult<Transaction> {
    let mut tx = Transaction::new();

    if let Some(version) = json
        .get("version")
        .and_then(neo_serialization::json::JToken::as_number)
    {
        tx.set_version(version as u8);
    }

    if let Some(nonce_token) = json.get("nonce") {
        let nonce = if let Some(number) = nonce_token.as_number() {
            number as u32
        } else if let Some(text) = nonce_token.as_string() {
            text.parse::<u32>()
                .map_err(|err| CoreError::other(format!("Invalid nonce value: {err}")))?
        } else {
            return Err(CoreError::other("Invalid 'nonce' field"));
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

    let parsed_signers = parse_optional_token_array_strict(
        json,
        "signers",
        "Signer entry must be an object",
        |token| {
            let signer_json = jtoken_to_serde(token)?;
            Signer::from_json(&signer_json)
                .map_err(|err| CoreError::other(format!("Invalid signer entry: {err}")))
        },
    )?;
    if !parsed_signers.is_empty() {
        tx.set_signers(parsed_signers);
    }

    let attributes = parse_optional_token_array_strict(
        json,
        "attributes",
        "Transaction attribute must be an object",
        |token| {
            let attr_obj = token
                .as_object()
                .ok_or_else(|| CoreError::other("Transaction attribute must be an object"))?;
            attribute_from_json(attr_obj)
        },
    )?;
    if !attributes.is_empty() {
        tx.set_attributes(attributes);
    }

    if let Some(script_token) = json.get("script") {
        let script_str = script_token
            .as_string()
            .ok_or_else(|| CoreError::other("Missing or invalid 'script' field"))?;
        let script_bytes = general_purpose::STANDARD
            .decode(script_str.as_bytes())
            .map_err(|err| CoreError::other(format!("Invalid 'script' value: {err}")))?;
        tx.set_script(script_bytes);
    }

    let witnesses = parse_optional_token_array_strict(
        json,
        "witnesses",
        "Witness entry must be an object",
        |token| {
            let witness_obj = token
                .as_object()
                .ok_or_else(|| CoreError::other("Witness entry must be an object"))?;
            payload_witness_from_json(witness_obj)
        },
    )?;
    if !witnesses.is_empty() {
        tx.set_witnesses(witnesses);
    }

    Ok(tx)
}

fn signer_to_json(signer: &neo_payloads::Signer, _protocol_settings: &ProtocolSettings) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "account".to_string(),
        JToken::String(signer.account.to_string()),
    );
    json.insert(
        "scopes".to_string(),
        JToken::String(signer.scopes.to_string()),
    );

    if !signer.allowed_contracts.is_empty() {
        json.insert(
            "allowedcontracts".to_string(),
            token_array(&signer.allowed_contracts, |contract| {
                JToken::String(contract.to_string())
            }),
        );
    }

    if !signer.allowed_groups.is_empty() {
        json.insert(
            "allowedgroups".to_string(),
            token_array(&signer.allowed_groups, |group| {
                JToken::String(hex::encode(group.to_bytes()))
            }),
        );
    }

    if !signer.rules.is_empty() {
        json.insert(
            "rules".to_string(),
            object_array(&signer.rules, rule_to_json),
        );
    }

    json
}

fn attribute_to_json(attr: &neo_payloads::TransactionAttribute) -> JObject {
    let mut json = JObject::new();
    json.insert(
        "type".to_string(),
        JToken::String(attr.type_id().to_string()),
    );
    // Add attribute-specific data based on type
    use neo_payloads::TransactionAttribute as TA;
    match attr {
        TA::HighPriority => {}
        TA::NotValidBefore(not_valid_before) => {
            json.insert(
                "height".to_string(),
                JToken::Number(f64::from(not_valid_before.height)),
            );
        }
        TA::Conflicts(conflicts) => {
            json.insert(
                "hash".to_string(),
                JToken::String(conflicts.hash.to_string()),
            );
        }
        TA::NotaryAssisted(notary) => {
            json.insert("nkeys".to_string(), JToken::Number(f64::from(notary.nkeys)));
        }
        TA::OracleResponse(response) => {
            json.insert("id".to_string(), JToken::Number(response.id as f64));
            json.insert(
                "code".to_string(),
                JToken::String(oracle_response_code_to_str(response.code).to_string()),
            );
            json.insert(
                "result".to_string(),
                JToken::String(general_purpose::STANDARD.encode(&response.result)),
            );
        }
    }
    json
}

fn rule_to_json(rule: &neo_payloads::WitnessRule) -> JObject {
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

fn condition_to_json(condition: &neo_payloads::WitnessCondition) -> JObject {
    use neo_payloads::WitnessCondition as WC;

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
            json.insert(
                "expressions".to_string(),
                object_array(conditions, condition_to_json),
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
