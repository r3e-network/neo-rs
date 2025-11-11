use base64::{engine::general_purpose, Engine as _};
use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToHex};
use neo_base::hash::Hash160;
use neo_crypto::ecc256::PublicKey;
use neo_crypto::{Secp256r1Verify, SignatureBytes};
use serde::{Deserialize, Serialize};

/// Represents a trusted contract group (mirrors C# `ContractGroup`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractGroup {
    pub public_key: PublicKey,
    pub signature: SignatureBytes,
}

impl ContractGroup {
    pub fn new(public_key: PublicKey, signature: SignatureBytes) -> Self {
        Self {
            public_key,
            signature,
        }
    }

    pub fn verify_contract(&self, contract_hash: &Hash160) -> bool {
        self.public_key
            .secp256r1_verify(contract_hash.as_slice(), &self.signature)
            .is_ok()
    }

    pub fn signature_bytes(&self) -> &[u8] {
        self.signature.as_ref()
    }
}

impl NeoEncode for ContractGroup {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.public_key.neo_encode(writer);
        self.signature.neo_encode(writer);
    }
}

impl NeoDecode for ContractGroup {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            public_key: PublicKey::neo_decode(reader)?,
            signature: SignatureBytes::neo_decode(reader)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ContractGroupSerde {
    #[serde(rename = "pubkey")]
    pub pubkey: String,
    pub signature: String,
}

impl Serialize for ContractGroup {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let helper = ContractGroupSerde {
            pubkey: self.public_key.to_compressed().to_hex_lower(),
            signature: general_purpose::STANDARD.encode(self.signature_bytes()),
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContractGroup {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = ContractGroupSerde::deserialize(deserializer)?;
        let key_bytes =
            hex::decode(helper.pubkey).map_err(|e| serde::de::Error::custom(e.to_string()))?;
        let public_key = PublicKey::from_sec1_bytes(&key_bytes)
            .map_err(|_| serde::de::Error::custom("invalid group public key"))?;
        let signature_bytes = general_purpose::STANDARD
            .decode(helper.signature.as_bytes())
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        if signature_bytes.len() != neo_crypto::ecdsa::SIGNATURE_SIZE {
            return Err(serde::de::Error::custom("invalid signature length"));
        }
        let mut buf = [0u8; neo_crypto::ecdsa::SIGNATURE_SIZE];
        buf.copy_from_slice(&signature_bytes);
        Ok(Self {
            public_key,
            signature: SignatureBytes(buf),
        })
    }
}
