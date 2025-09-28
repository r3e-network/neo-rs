// Copyright (C) 2015-2025 The Neo Project.
//
// vote.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::io::{ISerializable, MemoryReader, BinaryWriter};
use serde::{Serialize, Deserialize};

/// Vote implementation for state service.
/// Matches C# Vote class exactly
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vote {
    /// Index of the validator.
    /// Matches C# ValidatorIndex field
    pub validator_index: i32,
    
    /// Index of the root.
    /// Matches C# RootIndex field
    pub root_index: u32,
    
    /// Signature of the vote.
    /// Matches C# Signature field
    pub signature: Vec<u8>,
}

impl Vote {
    /// Creates a new Vote instance.
    pub fn new() -> Self {
        Self {
            validator_index: 0,
            root_index: 0,
            signature: Vec::new(),
        }
    }
    
    /// Creates a new Vote with specified parameters.
    pub fn new_with_params(validator_index: i32, root_index: u32, signature: Vec<u8>) -> Self {
        Self {
            validator_index,
            root_index,
            signature,
        }
    }
}

impl ISerializable for Vote {
    fn size(&self) -> usize {
        4 + 4 + self.signature.len() + 1 // validator_index + root_index + signature + var size
    }
    
    fn serialize(&self, writer: &mut dyn std::io::Write) -> Result<(), String> {
        // Write validator index
        writer.write_all(&self.validator_index.to_le_bytes())?;
        
        // Write root index
        writer.write_all(&self.root_index.to_le_bytes())?;
        
        // Write signature with var size
        self.write_var_bytes(writer, &self.signature)?;
        
        Ok(())
    }
    
    fn deserialize(&mut self, reader: &mut dyn std::io::Read) -> Result<(), String> {
        // Read validator index
        let mut validator_index_bytes = [0u8; 4];
        reader.read_exact(&mut validator_index_bytes)?;
        self.validator_index = i32::from_le_bytes(validator_index_bytes);
        
        // Read root index
        let mut root_index_bytes = [0u8; 4];
        reader.read_exact(&mut root_index_bytes)?;
        self.root_index = u32::from_le_bytes(root_index_bytes);
        
        // Read signature with var size
        self.signature = self.read_var_bytes(reader)?;
        
        Ok(())
    }
}

impl Vote {
    /// Writes variable-length bytes to writer.
    /// Matches C# WriteVarBytes method
    fn write_var_bytes(&self, writer: &mut dyn std::io::Write, data: &[u8]) -> Result<(), String> {
        if data.len() < 0xFD {
            writer.write_all(&[data.len() as u8])?;
        } else if data.len() <= 0xFFFF {
            writer.write_all(&[0xFD])?;
            writer.write_all(&(data.len() as u16).to_le_bytes())?;
        } else if data.len() <= 0xFFFFFFFF {
            writer.write_all(&[0xFE])?;
            writer.write_all(&(data.len() as u32).to_le_bytes())?;
        } else {
            writer.write_all(&[0xFF])?;
            writer.write_all(&(data.len() as u64).to_le_bytes())?;
        }
        writer.write_all(data)?;
        Ok(())
    }
    
    /// Reads variable-length bytes from reader.
    /// Matches C# ReadVarMemory method
    fn read_var_bytes(&self, reader: &mut dyn std::io::Read) -> Result<Vec<u8>, String> {
        let mut length_byte = [0u8; 1];
        reader.read_exact(&mut length_byte)?;
        
        let length = match length_byte[0] {
            len if len < 0xFD => len as usize,
            0xFD => {
                let mut bytes = [0u8; 2];
                reader.read_exact(&mut bytes)?;
                u16::from_le_bytes(bytes) as usize
            },
            0xFE => {
                let mut bytes = [0u8; 4];
                reader.read_exact(&mut bytes)?;
                u32::from_le_bytes(bytes) as usize
            },
            0xFF => {
                let mut bytes = [0u8; 8];
                reader.read_exact(&mut bytes)?;
                u64::from_le_bytes(bytes) as usize
            },
            _ => return Err("Invalid var length prefix".to_string()),
        };
        
        let mut data = vec![0u8; length];
        reader.read_exact(&mut data)?;
        Ok(data)
    }
}

impl Default for Vote {
    fn default() -> Self {
        Self::new()
    }
}