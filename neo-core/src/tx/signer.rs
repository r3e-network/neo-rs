// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use crate::h160::H160;
use crate::tx::{WitnessRule, WitnessScopes};
use neo_crypto::ecc256::PublicKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerType {
    Account = 0x00,
    Contract = 0x01,
    Tx = 0x02,
}

#[derive(Debug, Clone)]
pub struct Signer {
    pub account: H160,

    pub scopes: WitnessScopes,

    // #[serde(rename = "allowedcontracts")]
    // #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_contract: Vec<H160>,

    // #[serde(rename = "allowedgroups")]
    // #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_groups: Vec<PublicKey>,

    // #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<WitnessRule>,
}
