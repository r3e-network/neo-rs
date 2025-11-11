use alloc::string::String;

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash160,
};

use crate::manifest::ContractGroup;

use super::{ContractPermissionDescriptor, WildcardContainer};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContractPermission {
    pub contract: ContractPermissionDescriptor,
    pub methods: WildcardContainer<String>,
}

impl ContractPermission {
    pub fn allow_all() -> Self {
        Self {
            contract: ContractPermissionDescriptor::wildcard(),
            methods: WildcardContainer::wildcard(),
        }
    }

    pub fn allows(&self, hash: &Hash160, method: &str, groups: &[ContractGroup]) -> bool {
        if !self.contract.matches_contract(hash, groups) {
            return false;
        }

        match &self.methods {
            WildcardContainer::Wildcard => true,
            WildcardContainer::List(methods) => methods.iter().any(|m| m == method),
        }
    }
}

impl NeoEncode for ContractPermission {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.contract.neo_encode(writer);
        self.methods.neo_encode(writer);
    }
}

impl NeoDecode for ContractPermission {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            contract: ContractPermissionDescriptor::neo_decode(reader)?,
            methods: WildcardContainer::neo_decode(reader)?,
        })
    }
}
