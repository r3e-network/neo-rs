//! JSON projection helpers mirroring `RestServerUtility.JTokens.cs`.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hex::encode_upper;
use neo_core::{
    transaction::{
        attributes::{TransactionAttribute, TransactionAttributeType},
        core::Transaction,
    },
    Signer, UInt160, Witness, WitnessCondition, WitnessConditionType, WitnessRule,
    WitnessRuleAction, WitnessScope,
};
// Removed neo_cryptography dependency - using external crypto crates directly
use neo_io::Serializable;
use neo_ledger::block::{Block, BlockHeader};
use serde_json::{json, Value};

/// Convert a block header into a JSON representation matching the C# node.
pub fn block_header_to_json(header: &BlockHeader) -> Value {
    let witness_json = header
        .witnesses
        .first()
        .map(witness_to_json)
        .unwrap_or(Value::Null);

    json!({
        "Timestamp": header.timestamp,
        "Version": header.version,
        "PrimaryIndex": header.primary_index,
        "Index": header.index,
        "Nonce": header.nonce,
        "Hash": header.hash().to_string(),
        "MerkleRoot": header.merkle_root.to_string(),
        "PrevHash": header.previous_hash.to_string(),
        "NextConsensus": header.next_consensus.to_string(),
        "Witness": witness_json,
        "Size": header.size(),
    })
}

/// Convert a witness into JSON (InvocationScript/VerificationScript base64 encoded).
pub fn witness_to_json(witness: &Witness) -> Value {
    let mut clone = witness.clone();
    let script_hash = clone.script_hash();
    json!({
        "InvocationScript": encode_bytes(witness.invocation_script()),
        "VerificationScript": encode_bytes(witness.verification_script()),
        "ScriptHash": script_hash.to_string(),
    })
}

/// Convert a block into JSON including confirmations and transactions.
pub fn block_to_json(block: &Block, current_index: Option<u32>) -> Value {
    let confirmations = current_index
        .and_then(|current| current.checked_sub(block.index()))
        .map(|delta| (delta as u64) + 1);

    json!({
        "Timestamp": block.header.timestamp,
        "Version": block.header.version,
        "PrimaryIndex": block.header.primary_index,
        "Index": block.header.index,
        "Nonce": block.header.nonce,
        "Hash": block.hash().to_string(),
        "MerkleRoot": block.header.merkle_root.to_string(),
        "PrevHash": block.header.previous_hash.to_string(),
        "NextConsensus": block.header.next_consensus.to_string(),
        "Witness": block.header.witnesses.first().map(witness_to_json).unwrap_or(Value::Null),
        "Size": block.size(),
        "Confirmations": confirmations,
        "Transactions": block
            .transactions
            .iter()
            .map(transaction_to_json)
            .collect::<Vec<_>>(),
    })
}

/// Convert a transaction into the JSON shape expected by REST clients.
pub fn transaction_to_json(tx: &Transaction) -> Value {
    let hash = tx
        .get_hash()
        .map(|h| h.to_string())
        .unwrap_or_else(|_| String::from("0x"));
    let sender = tx.sender().map(|s| s.to_string());
    let script = encode_bytes(tx.script());
    let size = tx.size();
    let fee_per_byte = if size > 0 {
        tx.network_fee() / size as i64
    } else {
        0
    };

    json!({
        "Hash": hash,
        "Sender": sender,
        "Script": script,
        "FeePerByte": fee_per_byte,
        "NetworkFee": tx.network_fee(),
        "SystemFee": tx.system_fee(),
        "Size": size,
        "Nonce": tx.nonce(),
        "Version": tx.version(),
        "ValidUntilBlock": tx.valid_until_block(),
        "Witnesses": tx.witnesses().iter().map(witness_to_json).collect::<Vec<_>>(),
        "Signers": tx.signers().iter().map(signer_to_json).collect::<Vec<_>>(),
        "Attributes": tx
            .attributes()
            .iter()
            .map(transaction_attribute_to_json)
            .collect::<Vec<_>>(),
    })
}

/// Convert a signer to JSON.
pub fn signer_to_json(signer: &Signer) -> Value {
    json!({
        "Rules": signer.rules.iter().map(witness_rule_to_json).collect::<Vec<_>>(),
        "Account": signer.account.to_string(),
        "AllowedContracts": signer
            .allowed_contracts
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>(),
        "AllowedGroups": signer
            .allowed_groups
            .iter()
            .map(|g| encode_group(g))
            .collect::<Vec<_>>(),
        "Scopes": format!("{}", signer.scopes),
    })
}

/// Convert a transaction attribute to JSON matching the C# implementation.
pub fn transaction_attribute_to_json(attribute: &TransactionAttribute) -> Value {
    let attribute_type = format!("{:?}", attribute.attribute_type());
    let size = attribute.size();

    match attribute {
        TransactionAttribute::HighPriority => json!({
            "Type": attribute_type,
            "Size": size,
        }),
        TransactionAttribute::OracleResponse { id, code, result } => json!({
            "Type": attribute_type,
            "Id": id,
            "Code": format!("{:?}", code),
            "Result": encode_bytes(result),
            "Size": size,
        }),
        TransactionAttribute::NotValidBefore { height } => json!({
            "Type": attribute_type,
            "Height": height,
            "Size": size,
        }),
        TransactionAttribute::Conflicts { hash } => json!({
            "Type": attribute_type,
            "Hash": hash.to_string(),
            "Size": size,
        }),
    }
}

/// Convert a witness rule into JSON.
pub fn witness_rule_to_json(rule: &WitnessRule) -> Value {
    json!({
        "Action": format!("{:?}", rule.action),
        "Condition": witness_condition_to_json(&rule.condition),
    })
}

/// Convert a witness condition into JSON.
pub fn witness_condition_to_json(condition: &WitnessCondition) -> Value {
    let condition_type = format!("{:?}", condition.condition_type());

    match condition {
        WitnessCondition::Boolean { value } => json!({
            "Type": condition_type,
            "Expression": value,
        }),
        WitnessCondition::Not { condition } => json!({
            "Type": condition_type,
            "Expression": witness_condition_to_json(condition),
        }),
        WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => json!({
            "Type": condition_type,
            "Expressions": conditions
                .iter()
                .map(witness_condition_to_json)
                .collect::<Vec<_>>(),
        }),
        WitnessCondition::ScriptHash { hash } => json!({
            "Type": condition_type,
            "Hash": hash.to_string(),
        }),
        WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => json!({
            "Type": condition_type,
            "Group": encode_group(group),
        }),
        WitnessCondition::CalledByEntry => json!({
            "Type": condition_type,
        }),
        WitnessCondition::CalledByContract { hash } => json!({
            "Type": condition_type,
            "Hash": hash.to_string(),
        }),
    }
}

fn encode_bytes(data: &[u8]) -> String {
    STANDARD.encode(data)
}

fn encode_group(bytes: &[u8]) -> String {
    if let Ok(point) = ECPoint::from_bytes(bytes) {
        if let Ok(encoded) = point.encode_point(true) {
            return encode_upper(encoded);
        }
    }
    encode_upper(bytes)
}
