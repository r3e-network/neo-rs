// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use neo_base::encoding::bin::*;
use serde::{Deserialize, Serialize};

use crate::{tx::*, types::H160, PublicKey};

pub const MAX_ALLOWED_GROUPS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerType {
    Account = 0x00,
    Contract = 0x01,
    Tx = 0x02,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Signer {
    pub account: H160,

    pub scopes: WitnessScopes,

    #[serde(rename = "allowedcontracts")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_contract: Vec<H160>,

    #[serde(rename = "allowedgroups")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_groups: Vec<PublicKey>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<WitnessRule>,
}

impl BinEncoder for Signer {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        self.account.encode_bin(w);
        self.scopes.encode_bin(w);

        if self.scopes.has_scope(WitnessScope::CustomContracts) {
            self.allowed_contract.encode_bin(w);
        }

        if self.scopes.has_scope(WitnessScope::CustomGroups) {
            self.allowed_groups.encode_bin(w);
        }

        if self.scopes.has_scope(WitnessScope::WitnessRules) {
            self.rules.encode_bin(w);
        }
    }

    fn bin_size(&self) -> usize {
        let mut size = self.account.bin_size() + self.scopes.bin_size();
        if self.scopes.has_scope(WitnessScope::CustomContracts) {
            size += self.allowed_contract.bin_size();
        }

        if self.scopes.has_scope(WitnessScope::CustomGroups) {
            size += self.allowed_groups.bin_size();
        }

        if self.scopes.has_scope(WitnessScope::WitnessRules) {
            size += self.rules.bin_size();
        }

        size
    }
}

impl BinDecoder for Signer {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let mut signer = Signer {
            account: BinDecoder::decode_bin(r)?,
            scopes: BinDecoder::decode_bin(r)?,
            allowed_contract: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        };

        if signer.scopes.has_scope(WitnessScope::CustomContracts) {
            signer.allowed_contract = BinDecoder::decode_bin(r)?;
        }

        if signer.scopes.has_scope(WitnessScope::CustomGroups) {
            signer.allowed_groups = BinDecoder::decode_bin(r)?;
        }

        if signer.scopes.has_scope(WitnessScope::WitnessRules) {
            signer.rules = BinDecoder::decode_bin(r)?;
        }

        Ok(signer)
    }
}
