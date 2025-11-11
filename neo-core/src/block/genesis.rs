// Copyright @ 2025 - Present, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

use neo_crypto::ecc256::PublicKey;

use crate::script::Script;
use crate::tx::Role;

#[allow(dead_code)]
#[derive(Debug)]
pub struct Genesis {
    // #[serde(rename = "Roles")]
    pub roles: HashMap<Role, Vec<PublicKey>>,

    // #[serde(rename = "Transaction")]
    pub tx: GenesisTx,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct GenesisTx {
    #[serde(rename = "Script")]
    pub script: Script,

    #[serde(rename = "SystemFee")]
    pub sysfee: u64,
}
