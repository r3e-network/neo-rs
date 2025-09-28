// Copyright (C) 2015-2025 The Neo Project.
//
// scrypt_parameters.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Represents the parameters of the SCrypt algorithm.
/// Matches C# ScryptParameters class exactly
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScryptParameters {
    /// CPU/Memory cost parameter. Must be larger than 1, a power of 2 and less than 2^(128 * r / 8).
    /// Matches C# N field
    pub n: i32,
    
    /// The block size, must be >= 1.
    /// Matches C# R field
    pub r: i32,
    
    /// Parallelization parameter. Must be a positive integer less than or equal to Int32.MaxValue / (128 * r * 8).
    /// Matches C# P field
    pub p: i32,
}

impl ScryptParameters {
    /// The default parameters used by NEP6Wallet.
    /// Matches C# Default property
    pub fn default() -> Self {
        Self {
            n: 16384,
            r: 8,
            p: 8,
        }
    }
    
    /// Initializes a new instance of the ScryptParameters class.
    /// Matches C# constructor exactly
    pub fn new(n: i32, r: i32, p: i32) -> Self {
        Self { n, r, p }
    }
    
    /// Converts the parameters from a JSON object.
    /// Matches C# FromJson method
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let n = json["n"].as_i64()
            .ok_or("Missing or invalid 'n' field")? as i32;
        let r = json["r"].as_i64()
            .ok_or("Missing or invalid 'r' field")? as i32;
        let p = json["p"].as_i64()
            .ok_or("Missing or invalid 'p' field")? as i32;
        
        Ok(Self::new(n, r, p))
    }
    
    /// Converts the parameters to a JSON object.
    /// Matches C# ToJson method
    pub fn to_json(&self) -> serde_json::Value {
        let mut json = serde_json::Map::new();
        json.insert("n".to_string(), serde_json::Value::Number(serde_json::Number::from(self.n)));
        json.insert("r".to_string(), serde_json::Value::Number(serde_json::Number::from(self.r)));
        json.insert("p".to_string(), serde_json::Value::Number(serde_json::Number::from(self.p)));
        serde_json::Value::Object(json)
    }
}

impl Default for ScryptParameters {
    fn default() -> Self {
        Self::default()
    }
}