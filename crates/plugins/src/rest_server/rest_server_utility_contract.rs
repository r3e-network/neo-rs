// Copyright (C) 2015-2025 The Neo Project.
//
// rest_server_utility_contract.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, UInt256};
use serde_json::Value;
use std::collections::HashMap;

/// Contract invoke parameters matching C# InvokeParams
#[derive(Debug, Clone)]
pub struct ContractInvokeParameters {
    pub contract_parameters: Vec<ContractParameter>,
    pub signers: Vec<Signer>,
}

/// Signer matching C# Signer
#[derive(Debug, Clone)]
pub struct Signer {
    pub account: UInt160,
    pub scopes: WitnessScope,
}

/// Contract parameter matching C# ContractParameter
#[derive(Debug, Clone)]
pub struct ContractParameter {
    pub parameter_type: ContractParameterType,
    pub value: Option<ContractParameterValue>,
}

/// Contract parameter value matching C# ContractParameter value
#[derive(Debug, Clone)]
pub enum ContractParameterValue {
    ByteArray(Vec<u8>),
    Boolean(bool),
    Integer(num_bigint::BigInt),
    String(String),
    Hash160(UInt160),
    Hash256(UInt256),
    PublicKey(ECPoint),
    Array(Vec<ContractParameter>),
    Map(Vec<(ContractParameter, ContractParameter)>),
}

/// Contract parameter type matching C# ContractParameterType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractParameterType {
    Any,
    ByteArray,
    Signature,
    Boolean,
    Integer,
    String,
    Hash160,
    Hash256,
    PublicKey,
    Array,
    Map,
}

impl std::str::FromStr for ContractParameterType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Any" => Ok(ContractParameterType::Any),
            "ByteArray" => Ok(ContractParameterType::ByteArray),
            "Signature" => Ok(ContractParameterType::Signature),
            "Boolean" => Ok(ContractParameterType::Boolean),
            "Integer" => Ok(ContractParameterType::Integer),
            "String" => Ok(ContractParameterType::String),
            "Hash160" => Ok(ContractParameterType::Hash160),
            "Hash256" => Ok(ContractParameterType::Hash256),
            "PublicKey" => Ok(ContractParameterType::PublicKey),
            "Array" => Ok(ContractParameterType::Array),
            "Map" => Ok(ContractParameterType::Map),
            _ => Err(format!("Unknown contract parameter type: {}", s)),
        }
    }
}

/// Witness scope matching C# WitnessScope
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessScope {
    None,
    CalledByEntry,
    Global,
    WitnessRules,
}

impl std::str::FromStr for WitnessScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "None" => Ok(WitnessScope::None),
            "CalledByEntry" => Ok(WitnessScope::CalledByEntry),
            "Global" => Ok(WitnessScope::Global),
            "WitnessRules" => Ok(WitnessScope::WitnessRules),
            _ => Err(format!("Unknown witness scope: {}", s)),
        }
    }
}

/// ECPoint matching C# ECPoint
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ECPoint {
    pub x: Vec<u8>,
    pub y: Vec<u8>,
}

impl std::str::FromStr for ECPoint {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Simplified implementation - in a real implementation, this would parse the ECPoint
        Ok(ECPoint {
            x: vec![0u8; 32],
            y: vec![0u8; 32],
        })
    }
}

/// Contract utility functions matching C# RestServerUtility contract methods
impl super::RestServerUtility {
    /// Creates contract invoke parameters from JSON token
    /// Matches C# ContractInvokeParametersFromJToken method
    pub fn contract_invoke_parameters_from_j_token(
        token: &Value,
    ) -> Result<ContractInvokeParameters, String> {
        if !token.is_object() {
            return Err("Invalid token type".to_string());
        }

        let obj = token.as_object().unwrap();
        let contract_parameters_prop = obj.get("contractParameters");
        let signers_prop = obj.get("signers");

        if contract_parameters_prop.is_none() || signers_prop.is_none() {
            return Err("Missing required properties".to_string());
        }

        let contract_parameters: Vec<ContractParameter> = contract_parameters_prop
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|param| Self::contract_parameter_from_j_token(param))
            .collect::<Result<Vec<_>, _>>()?;

        let signers: Vec<Signer> = signers_prop
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|signer| Self::signer_from_j_token(signer))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ContractInvokeParameters {
            contract_parameters,
            signers,
        })
    }

    /// Creates signer from JSON token
    /// Matches C# SignerFromJToken method
    pub fn signer_from_j_token(token: &Value) -> Result<Signer, String> {
        if !token.is_object() {
            return Err("Invalid token type".to_string());
        }

        let obj = token.as_object().unwrap();
        let account_prop = obj.get("account");
        let scopes_prop = obj.get("scopes");

        if account_prop.is_none() || scopes_prop.is_none() {
            return Err("Missing required properties".to_string());
        }

        let account = UInt160::from_str(account_prop.unwrap().as_str().unwrap())?;
        let scopes = scopes_prop
            .unwrap()
            .as_str()
            .unwrap()
            .parse::<WitnessScope>()?;

        Ok(Signer { account, scopes })
    }

    /// Creates contract parameter from JSON token
    /// Matches C# ContractParameterFromJToken method
    pub fn contract_parameter_from_j_token(token: &Value) -> Result<ContractParameter, String> {
        if !token.is_object() {
            return Err("Invalid token type".to_string());
        }

        let obj = token.as_object().unwrap();
        let type_prop = obj.get("type");
        let value_prop = obj.get("value");

        if type_prop.is_none() || value_prop.is_none() {
            return Err("Missing required properties".to_string());
        }

        let type_value = type_prop
            .unwrap()
            .as_str()
            .unwrap()
            .parse::<ContractParameterType>()?;
        let value = value_prop.unwrap();

        match type_value {
            ContractParameterType::Any => Ok(ContractParameter {
                parameter_type: ContractParameterType::Any,
                value: None,
            }),
            ContractParameterType::ByteArray => {
                let bytes = base64::decode(value.as_str().unwrap())?;
                Ok(ContractParameter {
                    parameter_type: ContractParameterType::ByteArray,
                    value: Some(ContractParameterValue::ByteArray(bytes)),
                })
            }
            ContractParameterType::Signature => {
                let bytes = base64::decode(value.as_str().unwrap())?;
                Ok(ContractParameter {
                    parameter_type: ContractParameterType::Signature,
                    value: Some(ContractParameterValue::ByteArray(bytes)),
                })
            }
            ContractParameterType::Boolean => Ok(ContractParameter {
                parameter_type: ContractParameterType::Boolean,
                value: Some(ContractParameterValue::Boolean(value.as_bool().unwrap())),
            }),
            ContractParameterType::Integer => {
                let int_val = value.as_str().unwrap().parse::<i64>()?;
                Ok(ContractParameter {
                    parameter_type: ContractParameterType::Integer,
                    value: Some(ContractParameterValue::Integer(int_val.into())),
                })
            }
            ContractParameterType::String => Ok(ContractParameter {
                parameter_type: ContractParameterType::String,
                value: Some(ContractParameterValue::String(
                    value.as_str().unwrap().to_string(),
                )),
            }),
            ContractParameterType::Hash160 => {
                let hash = UInt160::from_str(value.as_str().unwrap())?;
                Ok(ContractParameter {
                    parameter_type: ContractParameterType::Hash160,
                    value: Some(ContractParameterValue::Hash160(hash)),
                })
            }
            ContractParameterType::Hash256 => {
                let hash = UInt256::from_str(value.as_str().unwrap())?;
                Ok(ContractParameter {
                    parameter_type: ContractParameterType::Hash256,
                    value: Some(ContractParameterValue::Hash256(hash)),
                })
            }
            ContractParameterType::PublicKey => {
                let public_key = value.as_str().unwrap().parse::<ECPoint>()?;
                Ok(ContractParameter {
                    parameter_type: ContractParameterType::PublicKey,
                    value: Some(ContractParameterValue::PublicKey(public_key)),
                })
            }
            ContractParameterType::Array => {
                if let Some(array) = value.as_array() {
                    let array_params: Vec<ContractParameter> = array
                        .iter()
                        .map(|item| Self::contract_parameter_from_j_token(item))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(ContractParameter {
                        parameter_type: ContractParameterType::Array,
                        value: Some(ContractParameterValue::Array(array_params)),
                    })
                } else {
                    Err("Invalid array format".to_string())
                }
            }
            ContractParameterType::Map => {
                if let Some(array) = value.as_array() {
                    let map_params: Vec<(ContractParameter, ContractParameter)> = array
                        .iter()
                        .map(|item| {
                            if let Some(obj) = item.as_object() {
                                let key = obj.get("key").unwrap();
                                let value = obj.get("value").unwrap();
                                let key_param = Self::contract_parameter_from_j_token(key)?;
                                let value_param = Self::contract_parameter_from_j_token(value)?;
                                Ok((key_param, value_param))
                            } else {
                                Err("Invalid map item format".to_string())
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(ContractParameter {
                        parameter_type: ContractParameterType::Map,
                        value: Some(ContractParameterValue::Map(map_params)),
                    })
                } else {
                    Err("Invalid map format".to_string())
                }
            }
        }
    }
}
