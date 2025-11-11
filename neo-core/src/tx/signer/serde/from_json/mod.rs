mod reader;
mod scopes;

use alloc::string::String;
use serde_json::Value;

use crate::{h160::H160, tx::WitnessScopes};

use super::super::Signer;
use super::parse::MAX_SUBITEMS;

pub fn signer_from_json(value: &Value) -> Result<Signer, String> {
    let reader = reader::SignerReader::new(value)?;
    let account = H160::try_from(reader.account)
        .map_err(|_| "Invalid signer account script hash".to_string())?;
    let scopes = reader
        .scopes
        .parse::<WitnessScopes>()
        .map_err(|e| alloc::format!("Invalid witness scope: {e}"))?;

    let mut signer = Signer {
        account,
        scopes,
        allowed_contract: alloc::vec::Vec::new(),
        allowed_groups: alloc::vec::Vec::new(),
        rules: alloc::vec::Vec::new(),
    };

    scopes::populate_scoped_fields(&reader.value, &mut signer, MAX_SUBITEMS)?;
    Ok(signer)
}
