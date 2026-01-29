use base64::{engine::general_purpose, Engine as _};
use neo_config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::{Block, BlockHeader, Signer, Transaction};
use neo_io::serializable::helper::get_var_size;
use neo_io::serializable::Serializable;
use neo_json::{JArray, JObject, JToken};

use super::attributes::attribute_from_json;
use super::parsing::{oracle_response_code_to_str, parse_i64_token, parse_u32_token};
use super::witness::{payload_witness_from_json, payload_witness_to_json, witness_to_json};

/// Converts a block to JSON representation.
pub fn block_to_json(block: &Block, protocol_settings: &ProtocolSettings) -> JObject {
    let mut json = JObject::new();
    let header = &block.header;

    json.insert("hash".to_string(), JToken::String(block.hash().to_string()));
    let block_size = header.size()
        + get_var_size(block.transactions.len() as u64)
        + block.transactions.iter().map(neo_io::Serializable::size).sum::<usize>();
    json.insert("size".to_string(), JToken::Number(block_size as f64));
    json.insert("version".to_string(), JToken::Number(f64::from(header.version)));
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
        JToken::String(format!("{:016X}", header.nonce)),
    );
    json.insert("index".to_string(), JToken::Number(f64::from(header.index)));
    json.insert(
        "primary".to_string(),
        JToken::Number(f64::from(header.primary_index)),
    );
    json.insert(
        "nextconsensus".to_string(),
        JToken::String(WalletHelper::to_address(
            &header.next_consensus,
            protocol_settings.address_version,
        )),
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
                .map(|tx| JToken::Object(transaction_to_json(tx, protocol_settings)))
                .collect(),
        ),
    );

    json
}

/// Converts JSON to a block
/// Matches C# `BlockFromJson`
pub fn block_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
    header_parser: fn(&JObject, &ProtocolSettings) -> Result<BlockHeader, String>,
) -> Result<Block, String> {
    let header = header_parser(json, protocol_settings)?;

    let transactions = json
        .get("tx")
        .and_then(|token| token.as_array())
        .map(|entries| {
            entries
                .children()
                .iter()
                .map(|entry| {
                    let obj = entry
                        .as_ref()
                        .and_then(|token| token.as_object())
                        .ok_or_else(|| "Transaction entry must be an object".to_string())?;
                    transaction_from_json(obj, protocol_settings)
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))?;

    Ok(Block::new(header, transactions))
}

/// Converts a transaction to JSON
/// Matches C# `TransactionToJson`
pub fn transaction_to_json(tx: &Transaction, protocol_settings: &ProtocolSettings) -> JObject {
    let mut json = JObject::new();

    json.insert("hash".to_string(), JToken::String(tx.hash().to_string()));
    json.insert("size".to_string(), JToken::Number(tx.size() as f64));
    json.insert("version".to_string(), JToken::Number(f64::from(tx.version())));
    json.insert("nonce".to_string(), JToken::Number(f64::from(tx.nonce())));
    json.insert(
        "sender".to_string(),
        tx.sender()
            .map_or(JToken::Null, |sender| {
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
        JToken::String(general_purpose::STANDARD.encode(tx.script())),
    );

    // Add witnesses
    json.insert(
        "witnesses".to_string(),
        JToken::Array(
            tx.witnesses()
                .iter()
                .map(|w| JToken::Object(payload_witness_to_json(w)))
                .collect(),
        ),
    );

    json
}

/// Converts JSON to a transaction
/// Matches C# `TransactionFromJson`
pub fn transaction_from_json(
    json: &JObject,
    _protocol_settings: &ProtocolSettings,
) -> Result<Transaction, String> {
    let mut tx = Transaction::new();

    if let Some(version) = json.get("version").and_then(neo_json::JToken::as_number) {
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
        for entry in signers_token.children() {
            let signer_token = entry
                .as_ref()
                .ok_or_else(|| "Signer entry must be an object".to_string())?;
            let signer_json = super::parsing::jtoken_to_serde(signer_token)?;
            parsed_signers.push(
                Signer::from_json(&signer_json)
                    .map_err(|err| format!("Invalid signer entry: {err}"))?,
            );
        }
        tx.set_signers(parsed_signers);
    }

    if let Some(attributes_token) = json.get("attributes").and_then(|t| t.as_array()) {
        let mut attributes = Vec::with_capacity(attributes_token.len());
        for entry in attributes_token.children() {
            let attr_obj = entry
                .as_ref()
                .and_then(|token| token.as_object())
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
        for entry in witnesses_token.children() {
            let witness_obj = entry
                .as_ref()
                .and_then(|token| token.as_object())
                .ok_or_else(|| "Witness entry must be an object".to_string())?;
            witnesses.push(payload_witness_from_json(witness_obj)?);
        }
        tx.set_witnesses(witnesses);
    }

    Ok(tx)
}

fn signer_to_json(signer: &neo_core::Signer, _protocol_settings: &ProtocolSettings) -> JObject {
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
                    .map(|g| JToken::String(hex::encode(g.to_bytes())))
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
    use neo_core::TransactionAttribute as TA;
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
