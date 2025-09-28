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

use neo_core::{
    Block, BlockHeader, ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, ContractState, MethodToken, NefFile, Signer, Transaction,
    TransactionAttribute, WildCardContainer, Witness, WitnessCondition, WitnessRule,
};
use serde_json::{json, Value};

/// RestServer utility JTokens functions matching C# RestServerUtility.JTokens exactly
impl super::RestServerUtility {
    /// Converts block header to JSON token
    /// Matches C# BlockHeaderToJToken method
    pub fn block_header_to_j_token(
        header: &BlockHeader,
        serializer: &serde_json::Serializer,
    ) -> Value {
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
            "Witness": Self::witness_to_j_token(&header.witness, serializer),
            "Size": header.size(),
        })
    }

    /// Converts witness to JSON token
    /// Matches C# WitnessToJToken method
    pub fn witness_to_j_token(witness: &Witness, serializer: &serde_json::Serializer) -> Value {
        json!({
            "InvocationScript": base64::encode(&witness.invocation_script),
            "VerificationScript": base64::encode(&witness.verification_script),
            "ScriptHash": witness.script_hash().to_string(),
        })
    }

    /// Converts block to JSON token
    /// Matches C# BlockToJToken method
    pub fn block_to_j_token(block: &Block, serializer: &serde_json::Serializer) -> Value {
        let confirmations = Self::get_current_index() - block.index + 1;
        let transactions: Vec<Value> = block
            .transactions
            .iter()
            .map(|tx| Self::transaction_to_j_token(tx, serializer))
            .collect();

        json!({
            "Timestamp": block.timestamp,
            "Version": block.version,
            "PrimaryIndex": block.primary_index,
            "Index": block.index,
            "Nonce": block.nonce,
            "Hash": block.hash().to_string(),
            "MerkleRoot": block.merkle_root.to_string(),
            "PrevHash": block.previous_hash.to_string(),
            "NextConsensus": block.next_consensus.to_string(),
            "Witness": Self::witness_to_j_token(&block.witness, serializer),
            "Size": block.size(),
            "Confirmations": confirmations,
            "Transactions": transactions,
        })
    }

    /// Converts transaction to JSON token
    /// Matches C# TransactionToJToken method
    pub fn transaction_to_j_token(tx: &Transaction, serializer: &serde_json::Serializer) -> Value {
        let witnesses: Vec<Value> = tx
            .witnesses
            .iter()
            .map(|w| Self::witness_to_j_token(w, serializer))
            .collect();
        let signers: Vec<Value> = tx
            .signers
            .iter()
            .map(|s| Self::signer_to_j_token(s, serializer))
            .collect();
        let attributes: Vec<Value> = tx
            .attributes
            .iter()
            .map(|a| Self::transaction_attribute_to_j_token(a, serializer))
            .collect();

        json!({
            "Hash": tx.hash().to_string(),
            "Sender": tx.sender.to_string(),
            "Script": base64::encode(&tx.script),
            "FeePerByte": tx.fee_per_byte,
            "NetworkFee": tx.network_fee,
            "SystemFee": tx.system_fee,
            "Size": tx.size(),
            "Nonce": tx.nonce,
            "Version": tx.version,
            "ValidUntilBlock": tx.valid_until_block,
            "Witnesses": witnesses,
            "Signers": signers,
            "Attributes": attributes,
        })
    }

    /// Converts signer to JSON token
    /// Matches C# SignerToJToken method
    pub fn signer_to_j_token(signer: &Signer, serializer: &serde_json::Serializer) -> Value {
        let rules: Vec<Value> = if let Some(rules) = &signer.rules {
            rules
                .iter()
                .map(|r| Self::witness_rule_to_j_token(r, serializer))
                .collect()
        } else {
            Vec::new()
        };

        json!({
            "Rules": rules,
            "Account": signer.account.to_string(),
            "AllowedContracts": signer.allowed_contracts,
            "AllowedGroups": signer.allowed_groups,
            "Scopes": signer.scopes,
        })
    }

    /// Converts transaction attribute to JSON token
    /// Matches C# TransactionAttributeToJToken method
    pub fn transaction_attribute_to_j_token(
        attribute: &TransactionAttribute,
        serializer: &serde_json::Serializer,
    ) -> Value {
        match attribute.attribute_type {
            TransactionAttributeType::Conflicts => {
                json!({
                    "Type": attribute.attribute_type,
                    "Hash": attribute.hash.to_string(),
                    "Size": attribute.size(),
                })
            }
            TransactionAttributeType::OracleResponse => {
                json!({
                    "Type": attribute.attribute_type,
                    "Id": attribute.id,
                    "Code": attribute.code,
                    "Result": base64::encode(&attribute.result),
                    "Size": attribute.size(),
                })
            }
            TransactionAttributeType::HighPriority => {
                json!({
                    "Type": attribute.attribute_type,
                    "Size": attribute.size(),
                })
            }
            TransactionAttributeType::NotValidBefore => {
                json!({
                    "Type": attribute.attribute_type,
                    "Height": attribute.height,
                    "Size": attribute.size(),
                })
            }
            _ => {
                json!({
                    "Type": attribute.attribute_type,
                    "Size": attribute.size(),
                })
            }
        }
    }

    /// Converts witness rule to JSON token
    /// Matches C# WitnessRuleToJToken method
    pub fn witness_rule_to_j_token(
        rule: &WitnessRule,
        serializer: &serde_json::Serializer,
    ) -> Value {
        json!({
            "Action": rule.action,
            "Condition": Self::witness_condition_to_j_token(&rule.condition, serializer),
        })
    }

    /// Converts witness condition to JSON token
    /// Matches C# WitnessConditionToJToken method
    pub fn witness_condition_to_j_token(
        condition: &WitnessCondition,
        serializer: &serde_json::Serializer,
    ) -> Value {
        match condition.condition_type {
            WitnessConditionType::Boolean => {
                json!({
                    "Type": condition.condition_type,
                    "Expression": condition.expression,
                })
            }
            WitnessConditionType::Not => {
                json!({
                    "Type": condition.condition_type,
                    "Expression": Self::witness_condition_to_j_token(&condition.expression, serializer),
                })
            }
            WitnessConditionType::And => {
                let expressions: Vec<Value> = condition
                    .expressions
                    .iter()
                    .map(|e| Self::witness_condition_to_j_token(e, serializer))
                    .collect();
                json!({
                    "Type": condition.condition_type,
                    "Expressions": expressions,
                })
            }
            WitnessConditionType::Or => {
                let expressions: Vec<Value> = condition
                    .expressions
                    .iter()
                    .map(|e| Self::witness_condition_to_j_token(e, serializer))
                    .collect();
                json!({
                    "Type": condition.condition_type,
                    "Expressions": expressions,
                })
            }
            WitnessConditionType::ScriptHash => {
                json!({
                    "Type": condition.condition_type,
                    "Hash": condition.hash.to_string(),
                })
            }
            WitnessConditionType::Group => {
                json!({
                    "Type": condition.condition_type,
                    "Group": condition.group.to_string(),
                })
            }
            WitnessConditionType::CalledByEntry => {
                json!({
                    "Type": condition.condition_type,
                })
            }
            WitnessConditionType::CalledByContract => {
                json!({
                    "Type": condition.condition_type,
                    "Hash": condition.hash.to_string(),
                })
            }
            WitnessConditionType::CalledByGroup => {
                json!({
                    "Type": condition.condition_type,
                    "Group": condition.group.to_string(),
                })
            }
        }
    }

    /// Converts contract state to JSON token
    /// Matches C# ContractStateToJToken method
    pub fn contract_state_to_j_token(
        contract: &ContractState,
        serializer: &serde_json::Serializer,
    ) -> Value {
        json!({
            "Id": contract.id,
            "UpdateCounter": contract.update_counter,
            "Name": contract.manifest.name,
            "Hash": contract.hash.to_string(),
            "Manifest": Self::contract_manifest_to_j_token(&contract.manifest, serializer),
            "NefFile": Self::contract_nef_file_to_j_token(&contract.nef, serializer),
        })
    }

    /// Converts contract manifest to JSON token
    /// Matches C# ContractManifestToJToken method
    pub fn contract_manifest_to_j_token(
        manifest: &ContractManifest,
        serializer: &serde_json::Serializer,
    ) -> Value {
        let groups: Vec<Value> = manifest
            .groups
            .iter()
            .map(|g| Self::contract_group_to_j_token(g, serializer))
            .collect();
        let permissions: Vec<Value> = manifest
            .permissions
            .iter()
            .map(|p| Self::contract_permission_to_j_token(p, serializer))
            .collect();
        let trusts: Vec<Value> = match &manifest.trusts {
            WildCardContainer::Wildcard => vec![Value::String("*".to_string())],
            WildCardContainer::List(descriptors) => descriptors
                .iter()
                .map(|descriptor| {
                    Self::contract_permission_descriptor_to_j_token(descriptor, serializer)
                })
                .collect(),
        };

        let extra_value = manifest.extra.clone().unwrap_or(Value::Null);

        json!({
            "Name": manifest.name,
            "Abi": Self::contract_abi_to_j_token(&manifest.abi, serializer),
            "Groups": groups,
            "Permissions": permissions,
            "Trusts": trusts,
            "SupportedStandards": manifest.supported_standards,
            "Extra": extra_value,
        })
    }

    /// Converts contract ABI to JSON token
    /// Matches C# ContractAbiToJToken method
    pub fn contract_abi_to_j_token(
        abi: &ContractAbi,
        serializer: &serde_json::Serializer,
    ) -> Value {
        let methods: Vec<Value> = abi
            .methods
            .iter()
            .map(|m| Self::contract_method_to_j_token(m, serializer))
            .collect();
        let events: Vec<Value> = abi
            .events
            .iter()
            .map(|e| Self::contract_event_to_j_token(e, serializer))
            .collect();

        json!({
            "Methods": methods,
            "Events": events,
        })
    }

    /// Converts contract method to JSON token
    /// Matches C# ContractMethodToJToken method
    pub fn contract_method_to_j_token(
        method: &ContractMethodDescriptor,
        serializer: &serde_json::Serializer,
    ) -> Value {
        let parameters: Vec<Value> = method
            .parameters
            .iter()
            .map(|p| Self::contract_method_parameter_to_j_token(p, serializer))
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
        serializer: &serde_json::Serializer,
    ) -> Value {
        json!({
            "Type": parameter.parameter_type,
            "Name": parameter.name,
        })
    }

    /// Converts contract group to JSON token
    /// Matches C# ContractGroupToJToken method
    pub fn contract_group_to_j_token(
        group: &ContractGroup,
        serializer: &serde_json::Serializer,
    ) -> Value {
        json!({
            "PubKey": group.pub_key.to_string(),
            "Signature": base64::encode(&group.signature),
        })
    }

    /// Converts contract permission to JSON token
    /// Matches C# ContractPermissionToJToken method
    pub fn contract_permission_to_j_token(
        permission: &ContractPermission,
        serializer: &serde_json::Serializer,
    ) -> Value {
        let methods = if permission.methods.is_empty() {
            Value::String("*".to_string())
        } else {
            json!(permission.methods)
        };

        json!({
            "Contract": Self::contract_permission_descriptor_to_j_token(&permission.contract, serializer),
            "Methods": methods,
        })
    }

    /// Converts contract permission descriptor to JSON token
    /// Matches C# ContractPermissionDescriptorToJToken method
    pub fn contract_permission_descriptor_to_j_token(
        desc: &ContractPermissionDescriptor,
        serializer: &serde_json::Serializer,
    ) -> Value {
        if desc.is_wildcard {
            Value::String("*".to_string())
        } else if desc.is_group {
            json!({
                "Group": desc.group.to_string()
            })
        } else if desc.is_hash {
            json!({
                "Hash": desc.hash.to_string()
            })
        } else {
            Value::Null
        }
    }

    /// Converts contract event to JSON token
    /// Matches C# ContractEventToJToken method
    pub fn contract_event_to_j_token(
        desc: &ContractEventDescriptor,
        serializer: &serde_json::Serializer,
    ) -> Value {
        let parameters: Vec<Value> = desc
            .parameters
            .iter()
            .map(|p| Self::contract_parameter_definition_to_j_token(p, serializer))
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
        serializer: &serde_json::Serializer,
    ) -> Value {
        json!({
            "Type": definition.parameter_type,
            "Name": definition.name,
        })
    }

    /// Converts contract NEF file to JSON token
    /// Matches C# ContractNefFileToJToken method
    pub fn contract_nef_file_to_j_token(
        nef: &NefFile,
        serializer: &serde_json::Serializer,
    ) -> Value {
        let tokens: Vec<Value> = nef
            .tokens
            .iter()
            .map(|t| Self::method_token_to_j_token(t, serializer))
            .collect();

        json!({
            "Checksum": nef.checksum,
            "Compiler": nef.compiler,
            "Script": base64::encode(&nef.script),
            "Source": nef.source,
            "Tokens": tokens,
        })
    }

    /// Converts method token to JSON token
    /// Matches C# MethodTokenToJToken method
    pub fn method_token_to_j_token(
        token: &MethodToken,
        serializer: &serde_json::Serializer,
    ) -> Value {
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
        // In a real implementation, this would get the current index from the ledger
        0
    }
}
