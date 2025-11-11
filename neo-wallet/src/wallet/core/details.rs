use alloc::{string::String, vec::Vec};

use neo_base::hash::Hash160;

use crate::signer::SignerScopes;

#[derive(Clone, Debug)]
pub struct AccountDetails {
    pub script_hash: Hash160,
    pub label: Option<String>,
    pub is_default: bool,
    pub lock: bool,
    pub scopes: SignerScopes,
    pub allowed_contracts: Vec<Hash160>,
    pub allowed_groups: Vec<Vec<u8>>,
}
