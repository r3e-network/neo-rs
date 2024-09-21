use serde::{Serialize, Deserialize};

/// Represents the parameters of the SCrypt algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScryptParameters {
    /// CPU/Memory cost parameter. Must be larger than 1, a power of 2 and less than 2^(128 * r / 8).
    pub n: u32,

    /// The block size, must be >= 1.
    pub r: u32,

    /// Parallelization parameter. Must be a positive integer less than or equal to u32::MAX / (128 * r * 8).
    pub p: u32,
}

impl Default for ScryptParameters {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ScryptParameters {
    /// The default parameters used by NEP6Wallet.
    pub const DEFAULT: Self = Self {
        n: 16384,
        r: 8,
        p: 8,
    };

    /// Initializes a new instance of the ScryptParameters struct.
    pub fn new(n: u32, r: u32, p: u32) -> Self {
        Self { n, r, p }
    }

    /// Converts the parameters from a JSON object.
    pub fn from_json(json: &JsonValue) -> Option<Self> {
        Some(Self {
            n: json["n"].as_u64()? as u32,
            r: json["r"].as_u64()? as u32,
            p: json["p"].as_u64()? as u32,
        })
    }

    /// Converts the parameters to a JSON object.
    pub fn to_json(&self) -> JsonValue {
        json!({
            "n": self.n,
            "r": self.r,
            "p": self.p,
        })
    }
}
