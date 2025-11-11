use alloc::vec::Vec;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::{
    h160::H160,
    io::{read_array, write_array},
    tx::{WitnessRule, WitnessScope, WitnessScopes},
};

use super::Signer;

impl NeoEncode for Signer {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(self.account.as_le_bytes());
        self.scopes.neo_encode(writer);
        if self.scopes.has_scope(WitnessScope::CustomContracts) {
            write_array(writer, &self.allowed_contract);
        }
        if self.scopes.has_scope(WitnessScope::CustomGroups) {
            write_array(writer, &self.allowed_groups);
        }
        if self.scopes.has_scope(WitnessScope::WitnessRules) {
            write_array(writer, &self.rules);
        }
    }
}

impl NeoDecode for Signer {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut account = [0u8; 20];
        reader.read_into(&mut account)?;
        let scopes = WitnessScopes::neo_decode(reader)?;
        let contracts = if scopes.has_scope(WitnessScope::CustomContracts) {
            read_array(reader)?
        } else {
            Vec::new()
        };
        let groups = if scopes.has_scope(WitnessScope::CustomGroups) {
            read_array(reader)?
        } else {
            Vec::new()
        };
        let rules: Vec<WitnessRule> = if scopes.has_scope(WitnessScope::WitnessRules) {
            read_array(reader)?
        } else {
            Vec::new()
        };
        Ok(Self {
            account: H160::from_le_bytes(account),
            scopes,
            allowed_contract: contracts,
            allowed_groups: groups,
            rules,
        })
    }
}
