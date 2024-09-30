// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
use neo_crypto::secp256r1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::Script;
use crate::dbft::ConsensusRole;

#[derive(Debug, Serialize, Deserialize)]
pub struct Genesis {
    #[serde(rename = "Roles")]
    pub roles: HashMap<ConsensusRole, Vec<PublicKey>>,

    #[serde(rename = "Transaction")]
    pub tx: GenesisTx,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenesisTx {
    #[serde(rename = "Script")]
    pub script: Script,

    #[serde(rename = "SystemFee")]
    pub sysfee: u64,
}
