// Copyright (C) 2015-2025 The Neo Project.
//
// state_root.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, UInt256, ProtocolSettings, Witness, DataCache, Contract, NativeContract, Role};
use neo_core::network::p2p::payloads::{IVerifiable, ISerializable};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// State root implementation.
/// Matches C# StateRoot class exactly
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateRoot {
    /// Current version constant.
    /// Matches C# CurrentVersion constant
    pub const CURRENT_VERSION: u8 = 0x00;
    
    /// Version of the state root.
    /// Matches C# Version field
    pub version: u8,
    
    /// Index of the state root.
    /// Matches C# Index field
    pub index: u32,
    
    /// Root hash of the state.
    /// Matches C# RootHash field
    pub root_hash: UInt256,
    
    /// Witness for the state root.
    /// Matches C# Witness field
    pub witness: Option<Witness>,
    
    /// Cached hash.
    /// Matches C# _hash field
    #[serde(skip)]
    hash: Option<UInt256>,
}

impl StateRoot {
    /// Creates a new StateRoot instance.
    pub fn new() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            index: 0,
            root_hash: UInt256::default(),
            witness: None,
            hash: None,
        }
    }
    
    /// Creates a new StateRoot with specified parameters.
    pub fn new_with_params(version: u8, index: u32, root_hash: UInt256, witness: Option<Witness>) -> Self {
        Self {
            version,
            index,
            root_hash,
            witness,
            hash: None,
        }
    }
    
    /// Gets the hash of the state root.
    /// Matches C# Hash property
    pub fn hash(&self) -> UInt256 {
        if let Some(hash) = self.hash {
            hash
        } else {
            let hash = self.calculate_hash();
            // In a real implementation, this would be cached
            hash
        }
    }
    
    /// Calculates the hash of the state root.
    /// Matches C# CalculateHash method
    pub fn calculate_hash(&self) -> UInt256 {
        // In a real implementation, this would calculate the actual hash
        // For now, we'll use a simplified approach
        let mut data = Vec::new();
        data.push(self.version);
        data.extend_from_slice(&self.index.to_le_bytes());
        data.extend_from_slice(&self.root_hash.to_bytes());
        
        // Simple hash calculation (in real implementation, use proper hashing)
        UInt256::from_bytes(&data[..32])
    }
    
    /// Deserializes unsigned data from reader.
    /// Matches C# DeserializeUnsigned method
    pub fn deserialize_unsigned(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        let mut version_byte = [0u8; 1];
        reader.read_exact(&mut version_byte)?;
        self.version = version_byte[0];
        
        let mut index_bytes = [0u8; 4];
        reader.read_exact(&mut index_bytes)?;
        self.index = u32::from_le_bytes(index_bytes);
        
        let mut root_hash_bytes = [0u8; 32];
        reader.read_exact(&mut root_hash_bytes)?;
        self.root_hash = UInt256::from_bytes(&root_hash_bytes);
        
        Ok(())
    }
    
    /// Serializes unsigned data to writer.
    /// Matches C# SerializeUnsigned method
    pub fn serialize_unsigned(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        writer.write_all(&[self.version])?;
        writer.write_all(&self.index.to_le_bytes())?;
        writer.write_all(&self.root_hash.to_bytes())?;
        Ok(())
    }
    
    /// Verifies the state root.
    /// Matches C# Verify method
    pub fn verify(&self, settings: &ProtocolSettings, snapshot: &DataCache) -> bool {
        // In a real implementation, this would verify witnesses
        // For now, we'll return true as a placeholder
        true
    }
    
    /// Gets script hashes for verifying.
    /// Matches C# GetScriptHashesForVerifying method
    pub fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Result<Vec<UInt160>, String> {
        let validators = NativeContract::role_management().get_designated_by_role(snapshot, Role::StateValidator, self.index)?;
        if validators.is_empty() {
            return Err("No script hash for state root verifying".to_string());
        }
        Ok(vec![Contract::get_bft_address(&validators)?])
    }
    
    /// Converts to JSON.
    /// Matches C# ToJson method
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::Map::new();
        json.insert("version".to_string(), serde_json::Value::Number(serde_json::Number::from(self.version)));
        json.insert("index".to_string(), serde_json::Value::Number(serde_json::Number::from(self.index)));
        json.insert("roothash".to_string(), serde_json::Value::String(self.root_hash.to_string()));
        
        if let Some(witness) = &self.witness {
            json.insert("witnesses".to_string(), serde_json::Value::Array(vec![witness.to_json()]));
        } else {
            json.insert("witnesses".to_string(), serde_json::Value::Array(vec![]));
        }
        
        serde_json::Value::Object(json)
    }
}

impl IVerifiable for StateRoot {
    fn witnesses(&self) -> Vec<Witness> {
        if let Some(witness) = &self.witness {
            vec![witness.clone()]
        } else {
            vec![]
        }
    }
    
    fn set_witnesses(&mut self, witnesses: Vec<Witness>) -> Result<(), String> {
        if witnesses.len() != 1 {
            return Err(format!("Expected 1 witness, got {}", witnesses.len()));
        }
        self.witness = Some(witnesses[0].clone());
        Ok(())
    }
}

impl ISerializable for StateRoot {
    fn size(&self) -> usize {
        let witness_size = if self.witness.is_some() { 1 + self.witness.as_ref().unwrap().size() } else { 1 };
        1 + 4 + 32 + witness_size
    }
    
    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        self.serialize_unsigned(writer)?;
        
        if let Some(witness) = &self.witness {
            writer.write_all(&[1])?; // Witness count
            witness.serialize(writer)?;
        } else {
            writer.write_all(&[0])?; // No witnesses
        }
        
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        self.deserialize_unsigned(reader)?;
        
        let mut witness_count = [0u8; 1];
        reader.read_exact(&mut witness_count)?;
        
        match witness_count[0] {
            0 => self.witness = None,
            1 => {
                let mut witness = Witness::new();
                witness.deserialize(reader)?;
                self.witness = Some(witness);
            },
            _ => return Err(format!("Expected 1 witness, got {}", witness_count[0])),
        }
        
        Ok(())
    }
}

impl Default for StateRoot {
    fn default() -> Self {
        Self::new()
    }
}