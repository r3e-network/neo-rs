mod accessors;
mod builder;

use neo_base::hash::Hash160;
use neo_crypto::ecc256::{PrivateKey, PublicKey};
use serde_json::Value;

use crate::{account::contract::Contract, signer::SignerScopes};

#[derive(Clone, Debug)]
pub struct Account {
    pub(crate) script_hash: Hash160,
    pub(crate) public_key: Option<PublicKey>,
    pub(crate) private_key: Option<PrivateKey>,
    pub(crate) label: Option<String>,
    pub(crate) is_default: bool,
    pub(crate) lock: bool,
    pub(crate) contract: Option<Contract>,
    pub(crate) extra: Option<Value>,
    pub(crate) signer_scopes: SignerScopes,
    pub(crate) allowed_contracts: Vec<Hash160>,
    pub(crate) allowed_groups: Vec<Vec<u8>>,
}
