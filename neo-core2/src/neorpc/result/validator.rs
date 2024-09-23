use serde::{Deserialize, Deserializer};
use serde_json::Error;
use crate::crypto::keys::PublicKey;

// Validator is used for the representation of consensus node data in the JSON-RPC protocol.
#[derive(Deserialize)]
pub struct Validator {
    #[serde(rename = "publickey")]
    public_key: PublicKey,
    #[serde(rename = "votes")]
    votes: i64,
}

// Candidate represents a node participating in the governance elections, it's active when it's a validator (consensus node).
#[derive(Deserialize)]
pub struct Candidate {
    #[serde(rename = "publickey")]
    public_key: PublicKey,
    #[serde(rename = "votes")]
    votes: i64,
    #[serde(rename = "active")]
    active: bool,
}

#[derive(Deserialize)]
struct NewValidator {
    #[serde(rename = "publickey")]
    public_key: PublicKey,
    #[serde(rename = "votes")]
    votes: i64,
}

#[derive(Deserialize)]
struct OldValidator {
    #[serde(rename = "publickey")]
    public_key: PublicKey,
    #[serde(rename = "votes")]
    votes: i64,
}

impl<'de> Deserialize<'de> for Validator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data: serde_json::Value = Deserialize::deserialize(deserializer)?;
        let nv: Result<NewValidator, Error> = serde_json::from_value(data.clone());
        if let Ok(nv) = nv {
            return Ok(Validator {
                public_key: nv.public_key,
                votes: nv.votes,
            });
        }
        let ov: Result<OldValidator, Error> = serde_json::from_value(data);
        if let Ok(ov) = ov {
            return Ok(Validator {
                public_key: ov.public_key,
                votes: ov.votes,
            });
        }
        Err(serde::de::Error::custom("Invalid data for Validator"))
    }
}
