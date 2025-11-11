use alloc::{collections::BTreeMap, string::String};

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::manifest::{ContractAbi, ContractFeatures, WildcardContainer};

use super::super::util::{decode_vec, encode_vec};
use super::model::{ContractManifest, MAX_MANIFEST_LENGTH};

impl NeoEncode for ContractManifest {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.name.neo_encode(writer);
        encode_vec(writer, &self.groups);
        self.features.neo_encode(writer);
        encode_vec(writer, &self.supported_standards);
        self.abi.neo_encode(writer);
        encode_vec(writer, &self.permissions);
        self.trusts.neo_encode(writer);
        let json = serde_json::to_vec(&self.extra).unwrap_or_default();
        writer.write_var_bytes(&json);
    }
}

impl NeoDecode for ContractManifest {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let name = String::neo_decode(reader)?;
        let groups = decode_vec(reader)?;
        let features = ContractFeatures::neo_decode(reader)?;
        let supported_standards = decode_vec(reader)?;
        let abi = ContractAbi::neo_decode(reader)?;
        let permissions = decode_vec(reader)?;
        let trusts = WildcardContainer::neo_decode(reader)?;
        let extra_raw = reader.read_var_bytes(MAX_MANIFEST_LENGTH as u64)?;
        let extra = if extra_raw.is_empty() {
            BTreeMap::new()
        } else {
            serde_json::from_slice(&extra_raw)
                .map_err(|_| DecodeError::InvalidValue("ContractManifest.extra"))?
        };

        Ok(Self {
            name,
            groups,
            features,
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        })
    }
}
