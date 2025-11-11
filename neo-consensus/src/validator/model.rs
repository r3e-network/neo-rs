use alloc::string::String;

use neo_crypto::ecc256::PublicKey;

use super::ValidatorId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validator {
    pub id: ValidatorId,
    pub public_key: PublicKey,
    pub alias: Option<String>,
}
