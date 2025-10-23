// Copyright (C) 2015-2025 The Neo Project.
//
// rest_server_utility_j_tokens.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use base64::{engine::general_purpose, Engine as _};
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use hex::encode as hex_encode;
use neo_core::network::p2p::payloads::{
    block::Block,
    header::Header as BlockHeader,
    signer::Signer,
    transaction::Transaction,
    transaction_attribute::TransactionAttribute,
    witness::Witness,
};
use neo_core::smart_contract::contract_state::{ContractState, MethodToken, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, WildCardContainer,
};
use neo_core::witness_rule::WitnessConditionType;
use neo_core::witness_rule::{WitnessCondition, WitnessRule};
use neo_core::prelude::Serializable;
use serde_json::{json, Value};
use tracing::warn;

/// RestServer utility JTokens functions matching C# RestServerUtility.JTokens exactly
impl super::RestServerUtility {
    /// Converts block header to JSON token
    /// Matches C# BlockHeaderToJToken method
    pub fn block_header_to_j_token(header: &BlockHeader) -> Value {
        let mut header_clone = header.clone();
        let hash = header_clone.hash();
        json!({
            "Timestamp": header.timestamp(),
            "Version": header.version(),
            "PrimaryIndex": header.primary_index(),
            "Index": header.index(),
            "Nonce": header.nonce(),
            "Hash": hash.to_string(),
            "MerkleRoot": header.merkle_root().to_string(),
            "PrevHash": header.prev_hash().to_string(),
            "NextConsensus": header.next_consensus().to_string(),
            "Witness": Self::witness_to_j_token(&header.witness),
            "Size": header.size(),
        })
    }

    /// Converts witness to JSON token
    /// Matches C# WitnessToJToken method
    pub fn witness_to_j_token(witness: &Witness) -> Value {
        json!({
            "InvocationScript": general_purpose::STANDARD.encode(&witness.invocation_script),
            "VerificationScript": general_purpose::STANDARD.encode(&witness.verification_script),
            "ScriptHash": witness.script_hash().to_string(),
        })
    }

    /// Converts block to JSON token
    /// Matches C# BlockToJToken method
    pub fn block_to_j_token(block: &Block) -> Value {
        let header = &block.header;
        let mut header_clone = header.clone();
        let hash = header_clone.hash();
        let confirmations = Self::get_current_index().saturating_sub(header.index()) + 1;
        let transactions: Vec<Value> = block
            .transactions
            .iter()
            .map(|tx| Self::transaction_to_j_token(tx))
            .collect();

        json!({
            "Timestamp": header.timestamp(),
            "Version": header.version(),
            "PrimaryIndex": header.primary_index(),
            "Index": header.index(),
            "Nonce": header.nonce(),
            "Hash": hash.to_string(),
            "MerkleRoot": header.merkle_root().to_string(),
            "PrevHash": header.prev_hash().to_string(),
            "NextConsensus": header.next_consensus().to_string(),
            "Witness": Self::witness_to_j_token(&header.witness),
            "Size": block.size(),
            "Confirmations": confirmations,
            "Transactions": transactions,
        })
    }

    /// Converts transaction to JSON token
    /// Matches C# TransactionToJToken method
    pub fn transaction_to_j_token(tx: &Transaction) -> Value {
        let witnesses: Vec<Value> = tx
            .witnesses()
            .iter()
            .map(|w| Self::witness_to_j_token(w))
            .collect();
        let signers: Vec<Value> = tx
            .signers()
            .iter()
            .map(|s| Self::signer_to_j_token(s))
            .collect();
        let attributes: Vec<Value> = tx
            .attributes()
            .iter()
            .map(|a| Self::transaction_attribute_to_j_token(a))
            .collect();

        json!({
            "Hash": tx.hash().to_string(),
            "Sender": tx.sender().map(|s| s.to_string()),
            "Script": general_purpose::STANDARD.encode(tx.script()),
            "FeePerByte": tx.fee_per_byte(),
            "NetworkFee": tx.network_fee(),
            "SystemFee": tx.system_fee(),
            "Size": tx.size(),
            "Nonce": tx.nonce(),
            "Version": tx.version(),
            "ValidUntilBlock": tx.valid_until_block(),
            "Witnesses": witnesses,
            "Signers": signers,
            "Attributes": attributes,
        })
    }

    /// Converts signer to JSON token
    /// Matches C# SignerToJToken method
    pub fn signer_to_j_token(signer: &Signer) -> Value {
        let rules: Vec<Value> = signer
            .rules
            .iter()
            .map(|rule| Self::witness_rule_to_j_token(rule))
            .collect();

        let allowed_contracts: Vec<String> = signer
            .allowed_contracts
            .iter()
            .map(|hash| hash.to_string())
            .collect();

        let allowed_groups: Vec<String> = signer
            .allowed_groups
            .iter()
            .map(|group| encode_with_0x(group.as_bytes()))
            .collect();

        json!({
            "Rules": rules,
            "Account": signer.account.to_string(),
            "AllowedContracts": allowed_contracts,
            "AllowedGroups": allowed_groups,
            "Scopes": signer.scopes,
        })
    }

    /// Converts transaction attribute to JSON token
    /// Matches C# TransactionAttributeToJToken method
    pub fn transaction_attribute_to_j_token(attribute: &TransactionAttribute) -> Value {
        match attribute {
            TransactionAttribute::Conflicts(conflict) => json!({
                "Type": attribute.attribute_type(),
                "Hash": conflict.hash.to_string(),
                "Size": attribute.size(),
            }),
            TransactionAttribute::OracleResponse(response) => json!({
                "Type": attribute.attribute_type(),
                "Id": response.id,
                "Code": response.code,
                "Result": general_purpose::STANDARD.encode(&response.result),
                "Size": attribute.size(),
            }),
            TransactionAttribute::HighPriority => json!({
                "Type": attribute.attribute_type(),
                "Size": attribute.size(),
            }),
            TransactionAttribute::NotValidBefore(attr) => json!({
                "Type": attribute.attribute_type(),
                "Height": attr.height,
                "Size": attribute.size(),
            }),
            TransactionAttribute::NotaryAssisted(attr) => json!({
                "Type": attribute.attribute_type(),
                "NKeys": attr.nkeys,
                "Size": attribute.size(),
            }),
        }
    }

    /// Converts witness rule to JSON token
    /// Matches C# WitnessRuleToJToken method
    pub fn witness_rule_to_j_token(rule: &WitnessRule) -> Value {
        json!({
            "Action": rule.action,
            "Condition": Self::witness_condition_to_j_token(&rule.condition),
        })
    }

    /// Converts witness condition to JSON token
    /// Matches C# WitnessConditionToJToken method
    pub fn witness_condition_to_j_token(condition: &WitnessCondition) -> Value {
        match condition {
            WitnessCondition::Boolean { value } => json!({
                "Type": WitnessConditionType::Boolean.to_string(),
                "Expression": value,
            }),
            WitnessCondition::Not { condition } => json!({
                "Type": WitnessConditionType::Not.to_string(),
                "Expression": Self::witness_condition_to_j_token(condition),
            }),
            WitnessCondition::And { conditions } => {
                let expressions: Vec<Value> = conditions
                    .iter()
                    .map(|condition| Self::witness_condition_to_j_token(condition))
                    .collect();
                json!({
                    "Type": WitnessConditionType::And.to_string(),
                    "Expressions": expressions,
                })
            }
            WitnessCondition::Or { conditions } => {
                let expressions: Vec<Value> = conditions
                    .iter()
                    .map(|condition| Self::witness_condition_to_j_token(condition))
                    .collect();
                json!({
                    "Type": WitnessConditionType::Or.to_string(),
                    "Expressions": expressions,
                })
            }
            WitnessCondition::ScriptHash { hash } => json!({
                "Type": WitnessConditionType::ScriptHash.to_string(),
                "Hash": hash.to_string(),
            }),
            WitnessCondition::Group { group } => json!({
                "Type": WitnessConditionType::Group.to_string(),
                "Group": encode_with_0x(group),
            }),
            WitnessCondition::CalledByEntry => json!({
                "Type": WitnessConditionType::CalledByEntry.to_string(),
            }),
            WitnessCondition::CalledByContract { hash } => json!({
                "Type": WitnessConditionType::CalledByContract.to_string(),
                "Hash": hash.to_string(),
            }),
            WitnessCondition::CalledByGroup { group } => json!({
                "Type": WitnessConditionType::CalledByGroup.to_string(),
                "Group": encode_with_0x(group),
            }),
        }
    }

    /// Converts contract state to JSON token
    /// Matches C# ContractStateToJToken method
    pub fn contract_state_to_j_token(contract: &ContractState) -> Value {
        json!({
            "Id": contract.id,
            "UpdateCounter": contract.update_counter,
            "Name": contract.manifest.name,
            "Hash": contract.hash.to_string(),
            "Manifest": Self::contract_manifest_to_j_token(&contract.manifest),
            "NefFile": Self::contract_nef_file_to_j_token(&contract.nef),
        })
    }

    /// Converts contract manifest to JSON token
    /// Matches C# ContractManifestToJToken method
    pub fn contract_manifest_to_j_token(manifest: &ContractManifest) -> Value {
        let groups: Vec<Value> = manifest
            .groups
            .iter()
            .map(|group| Self::contract_group_to_j_token(group))
            .collect();
        let permissions: Vec<Value> = manifest
            .permissions
            .iter()
            .map(|permission| Self::contract_permission_to_j_token(permission))
            .collect();
        let trusts: Vec<Value> = match &manifest.trusts {
            WildCardContainer::Wildcard => vec![Value::String("*".to_string())],
            WildCardContainer::List(descriptors) => descriptors
                .iter()
                .map(|descriptor| Self::contract_permission_descriptor_to_j_token(descriptor))
                .collect(),
        };

        let extra_value = manifest.extra.clone().unwrap_or(Value::Null);

        json!({
            "Name": manifest.name,
            "Abi": Self::contract_abi_to_j_token(&manifest.abi),
            "Groups": groups,
            "Permissions": permissions,
            "Trusts": trusts,
            "SupportedStandards": manifest.supported_standards,
            "Extra": extra_value,
        })
    }

    /// Converts contract ABI to JSON token
    /// Matches C# ContractAbiToJToken method
    pub fn contract_abi_to_j_token(abi: &ContractAbi) -> Value {
        let methods: Vec<Value> = abi
            .methods
            .iter()
            .map(|method| Self::contract_method_to_j_token(method))
            .collect();
        let events: Vec<Value> = abi
            .events
            .iter()
            .map(|event| Self::contract_event_to_j_token(event))
            .collect();

        json!({
            "Methods": methods,
            "Events": events,
        })
    }

    /// Converts contract method to JSON token
    /// Matches C# ContractMethodToJToken method
    pub fn contract_method_to_j_token(method: &ContractMethodDescriptor) -> Value {
        let parameters: Vec<Value> = method
            .parameters
            .iter()
            .map(|parameter| Self::contract_method_parameter_to_j_token(parameter))
            .collect();

        json!({
            "Name": method.name,
            "Safe": method.safe,
            "Offset": method.offset,
            "Parameters": parameters,
            "ReturnType": method.return_type,
        })
    }

    /// Converts contract method parameter to JSON token
    /// Matches C# ContractMethodParameterToJToken method
    pub fn contract_method_parameter_to_j_token(
        parameter: &ContractParameterDefinition,
    ) -> Value {
        json!({
            "Type": parameter.param_type,
            "Name": parameter.name,
        })
    }

    /// Converts contract group to JSON token
    /// Matches C# ContractGroupToJToken method
    pub fn contract_group_to_j_token(group: &ContractGroup) -> Value {
        json!({
            "PubKey": encode_with_0x(group.pub_key.as_bytes()),
            "Signature": general_purpose::STANDARD.encode(&group.signature),
        })
    }

    /// Converts contract permission to JSON token
    /// Matches C# ContractPermissionToJToken method
    pub fn contract_permission_to_j_token(permission: &ContractPermission) -> Value {
        let methods = match &permission.methods {
            WildCardContainer::Wildcard => Value::String("*".to_string()),
            WildCardContainer::List(list) => json!(list),
        };

        json!({
            "Contract": Self::contract_permission_descriptor_to_j_token(&permission.contract),
            "Methods": methods,
        })
    }

    /// Converts contract permission descriptor to JSON token
    /// Matches C# ContractPermissionDescriptorToJToken method
    pub fn contract_permission_descriptor_to_j_token(desc: &ContractPermissionDescriptor) -> Value {
        match desc {
            ContractPermissionDescriptor::Wildcard => Value::String("*".to_string()),
            ContractPermissionDescriptor::Group(group) => {
                json!({ "Group": encode_with_0x(group.as_bytes()) })
            }
            ContractPermissionDescriptor::Hash(hash) => {
                json!({ "Hash": hash.to_string() })
            }
        }
    }

    /// Converts contract event to JSON token
    /// Matches C# ContractEventToJToken method
    pub fn contract_event_to_j_token(desc: &ContractEventDescriptor) -> Value {
        let parameters: Vec<Value> = desc
            .parameters
            .iter()
            .map(|parameter| Self::contract_parameter_definition_to_j_token(parameter))
            .collect();

        json!({
            "Name": desc.name,
            "Parameters": parameters,
        })
    }

    /// Converts contract parameter definition to JSON token
    /// Matches C# ContractParameterDefinitionToJToken method
    pub fn contract_parameter_definition_to_j_token(
        definition: &ContractParameterDefinition,
    ) -> Value {
        json!({
            "Type": definition.param_type,
            "Name": definition.name,
        })
    }

    /// Converts contract NEF file to JSON token
    /// Matches C# ContractNefFileToJToken method
    pub fn contract_nef_file_to_j_token(nef: &NefFile) -> Value {
        let tokens: Vec<Value> = nef
            .tokens
            .iter()
            .map(|token| Self::method_token_to_j_token(token))
            .collect();

        json!({
            "Checksum": nef.checksum,
            "Compiler": nef.compiler,
            "Script": general_purpose::STANDARD.encode(&nef.script),
            "Source": nef.source,
            "Tokens": tokens,
        })
    }

    /// Converts method token to JSON token
    /// Matches C# MethodTokenToJToken method
    pub fn method_token_to_j_token(token: &MethodToken) -> Value {
        json!({
            "Hash": token.hash.to_string(),
            "Method": token.method,
            "CallFlags": token.call_flags,
            "ParametersCount": token.parameters_count,
            "HasReturnValue": token.has_return_value,
        })
    }

    /// Gets the current index
    /// Matches C# NativeContract.Ledger.CurrentIndex
    fn get_current_index() -> u32 {
        match RestServerGlobals::neo_system() {
            Some(system) => system.current_block_index(),
            None => {
                warn!("RestServerUtility: NeoSystem not initialised while resolving ledger height");
                0
            }
        }
    }
}

fn encode_with_0x(bytes: &[u8]) -> String {
    format!("0x{}", hex_encode(bytes))
}
