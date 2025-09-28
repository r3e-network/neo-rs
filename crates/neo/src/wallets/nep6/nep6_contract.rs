// Copyright (C) 2015-2025 The Neo Project.
//
// nep6_contract.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{
    smart_contract::{contract::Contract, contract_parameter_type::ContractParameterType},
    uint160::UInt160,
};

use super::super::Contract as BaseContract;

/// NEP6 contract implementation.
/// Matches C# NEP6Contract class exactly
pub struct NEP6Contract {
    /// Base contract functionality
    base: BaseContract,
    
    /// Parameter names
    /// Matches C# ParameterNames field
    pub parameter_names: Vec<String>,
    
    /// Deployment status
    /// Matches C# Deployed field
    pub deployed: bool,
}

impl NEP6Contract {
    /// Creates a new NEP6Contract instance.
    pub fn new() -> Self {
        Self {
            base: BaseContract::new(),
            parameter_names: Vec::new(),
            deployed: false,
        }
    }
    
    /// Creates an NEP6Contract from JSON.
    /// Matches C# FromJson method
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let mut contract = Self::new();
        
        if let Some(script_str) = json["script"].as_str() {
            let script = base64::decode(script_str)
                .map_err(|e| format!("Failed to decode script: {}", e))?;
            contract.base.set_script(script);
        }
        
        if let Some(parameters) = json["parameters"].as_array() {
            let mut parameter_list = Vec::new();
            let mut parameter_names = Vec::new();
            
            for param in parameters {
                if let Some(name) = param["name"].as_str() {
                    parameter_names.push(name.to_string());
                }
                
                if let Some(type_str) = param["type"].as_str() {
                    let param_type = match type_str {
                        "Signature" => ContractParameterType::Signature,
                        "Boolean" => ContractParameterType::Boolean,
                        "Integer" => ContractParameterType::Integer,
                        "Hash160" => ContractParameterType::Hash160,
                        "Hash256" => ContractParameterType::Hash256,
                        "ByteArray" => ContractParameterType::ByteArray,
                        "PublicKey" => ContractParameterType::PublicKey,
                        "String" => ContractParameterType::String,
                        "Array" => ContractParameterType::Array,
                        "Map" => ContractParameterType::Map,
                        "InteropInterface" => ContractParameterType::InteropInterface,
                        "Void" => ContractParameterType::Void,
                        _ => return Err(format!("Unknown parameter type: {}", type_str)),
                    };
                    parameter_list.push(param_type);
                }
            }
            
            contract.base.set_parameter_list(parameter_list);
            contract.parameter_names = parameter_names;
        }
        
        if let Some(deployed) = json["deployed"].as_bool() {
            contract.deployed = deployed;
        }
        
        Ok(contract)
    }
    
    /// Converts the contract to JSON.
    /// Matches C# ToJson method
    pub fn to_json(&self) -> serde_json::Value {
        let mut contract = serde_json::Map::new();
        
        contract.insert("script".to_string(), serde_json::Value::String(
            base64::encode(&self.base.script())
        ));
        
        let mut parameters = Vec::new();
        for (i, param_type) in self.base.parameter_list().iter().enumerate() {
            let mut parameter = serde_json::Map::new();
            
            if i < self.parameter_names.len() {
                parameter.insert("name".to_string(), serde_json::Value::String(
                    self.parameter_names[i].clone()
                ));
            }
            
            let type_str = match param_type {
                ContractParameterType::Signature => "Signature",
                ContractParameterType::Boolean => "Boolean",
                ContractParameterType::Integer => "Integer",
                ContractParameterType::Hash160 => "Hash160",
                ContractParameterType::Hash256 => "Hash256",
                ContractParameterType::ByteArray => "ByteArray",
                ContractParameterType::PublicKey => "PublicKey",
                ContractParameterType::String => "String",
                ContractParameterType::Array => "Array",
                ContractParameterType::Map => "Map",
                ContractParameterType::InteropInterface => "InteropInterface",
                ContractParameterType::Void => "Void",
                _ => "Unknown",
            };
            
            parameter.insert("type".to_string(), serde_json::Value::String(type_str.to_string()));
            parameters.push(serde_json::Value::Object(parameter));
        }
        
        contract.insert("parameters".to_string(), serde_json::Value::Array(parameters));
        contract.insert("deployed".to_string(), serde_json::Value::Bool(self.deployed));
        
        serde_json::Value::Object(contract)
    }
}

impl Default for NEP6Contract {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for NEP6Contract {
    type Target = BaseContract;
    
    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl std::ops::DerefMut for NEP6Contract {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}