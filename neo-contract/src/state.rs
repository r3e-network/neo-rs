use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash160,
};
use serde::{Deserialize, Serialize};

use crate::{manifest::ContractManifest, nef::NefFile};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractState {
    pub id: u32,
    pub update_counter: u32,
    pub hash: Hash160,
    #[serde(rename = "nef")]
    pub nef: NefFile,
    pub manifest: ContractManifest,
}

impl ContractState {
    pub fn new(id: u32, hash: Hash160, nef: NefFile, manifest: ContractManifest) -> Self {
        Self {
            id,
            update_counter: 0,
            hash,
            nef,
            manifest,
        }
    }
}

impl NeoEncode for ContractState {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u32(self.id);
        writer.write_u32(self.update_counter);
        self.hash.neo_encode(writer);
        self.nef.neo_encode(writer);
        self.manifest.neo_encode(writer);
    }
}

impl NeoDecode for ContractState {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let id = reader.read_u32()?;
        let update_counter = reader.read_u32()?;
        let hash = Hash160::neo_decode(reader)?;
        let nef = NefFile::neo_decode(reader)?;
        let manifest = ContractManifest::neo_decode(reader)?;
        Ok(Self {
            id,
            update_counter,
            hash,
            nef,
            manifest,
        })
    }
}
