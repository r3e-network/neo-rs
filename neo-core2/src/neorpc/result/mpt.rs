use serde::{Deserialize, Serialize};
use serde::de::{self, Deserializer};
use serde::ser::{Serializer, SerializeStruct};
use base64;
use std::fmt;
use crate::io::{BinWriter, BinReader, Serializable};

#[derive(Serialize, Deserialize)]
pub struct StateHeight {
    #[serde(rename = "localrootindex")]
    local: u32,
    #[serde(rename = "validatedrootindex")]
    validated: u32,
}

pub struct ProofWithKey {
    key: Vec<u8>,
    proof: Vec<Vec<u8>>,
}

impl Serialize for ProofWithKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ProofWithKey", 2)?;
        let mut w = BinWriter::new();
        self.encode_binary(&mut w);
        if let Some(err) = w.err() {
            return Err(serde::ser::Error::custom(err.to_string()));
        }
        let encoded = base64::encode(w.bytes());
        state.serialize_field("key", &encoded)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ProofWithKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        let mut p = ProofWithKey {
            key: Vec::new(),
            proof: Vec::new(),
        };
        p.from_string(&s).map_err(de::Error::custom)?;
        Ok(p)
    }
}

impl ProofWithKey {
    fn encode_binary(&self, w: &mut BinWriter) {
        w.write_var_bytes(&self.key);
        w.write_var_uint(self.proof.len() as u64);
        for p in &self.proof {
            w.write_var_bytes(p);
        }
    }

    fn decode_binary(&mut self, r: &mut BinReader) {
        self.key = r.read_var_bytes();
        let sz = r.read_var_uint();
        for _ in 0..sz {
            self.proof.push(r.read_var_bytes());
        }
    }

    fn from_string(&mut self, s: &str) -> Result<(), Box<dyn std::error::Error>> {
        let raw_proof = base64::decode(s)?;
        let mut r = BinReader::new(&raw_proof);
        self.decode_binary(&mut r);
        r.err().map_err(|e| e.to_string().into())
    }
}

impl fmt::Display for ProofWithKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut w = BinWriter::new();
        self.encode_binary(&mut w);
        write!(f, "{}", base64::encode(w.bytes()))
    }
}

pub struct VerifyProof {
    value: Option<Vec<u8>>,
}

impl Serialize for VerifyProof {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(ref value) = self.value {
            let encoded = base64::encode(value);
            serializer.serialize_str(&encoded)
        } else {
            serializer.serialize_str("invalid")
        }
    }
}

impl<'de> Deserialize<'de> for VerifyProof {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        if s == "invalid" {
            Ok(VerifyProof { value: None })
        } else {
            let decoded = base64::decode(&s).map_err(de::Error::custom)?;
            Ok(VerifyProof { value: Some(decoded) })
        }
    }
}
