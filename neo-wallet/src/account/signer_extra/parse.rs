use alloc::vec::Vec;

use core::str::FromStr;
use hex::decode;
use serde_json::Value;

use neo_base::hash::Hash160;

use crate::signer::SignerScopes;

pub(crate) fn parse_signer_extra(
    extra: Option<Value>,
) -> Result<(Option<Value>, SignerScopes, Vec<Hash160>, Vec<Vec<u8>>), crate::error::WalletError> {
    let mut scopes = SignerScopes::CALLED_BY_ENTRY;
    let mut allowed_contracts = Vec::new();
    let mut allowed_groups = Vec::new();

    if let Some(Value::Object(mut map)) = extra.clone() {
        if let Some(Value::Object(signer)) = map.remove("signer") {
            if let Some(Value::String(scopes_str)) = signer.get("scopes") {
                if let Some(parsed) = SignerScopes::from_witness_scope_string(scopes_str) {
                    if parsed.is_valid() {
                        scopes = parsed;
                    }
                }
            }

            if let Some(Value::Array(contracts)) = signer.get("allowedContracts") {
                for value in contracts {
                    let Some(str_val) = value.as_str() else {
                        return Err(crate::error::WalletError::InvalidNep6(
                            "allowedContracts entries must be strings",
                        ));
                    };
                    let hash = Hash160::from_str(str_val).map_err(|_| {
                        crate::error::WalletError::InvalidNep6(
                            "invalid script hash in allowedContracts",
                        )
                    })?;
                    allowed_contracts.push(hash);
                }
            }

            if let Some(Value::Array(groups)) = signer.get("allowedGroups") {
                for value in groups {
                    let Some(str_val) = value.as_str() else {
                        return Err(crate::error::WalletError::InvalidNep6(
                            "allowedGroups entries must be strings",
                        ));
                    };
                    let trimmed = str_val.strip_prefix("0x").unwrap_or(str_val);
                    let bytes = decode(trimmed).map_err(|_| {
                        crate::error::WalletError::InvalidNep6("invalid allowedGroups entry")
                    })?;
                    allowed_groups.push(bytes);
                }
            }
        }
        let remaining = if map.is_empty() {
            None
        } else {
            Some(Value::Object(map))
        };
        return Ok((remaining, scopes, allowed_contracts, allowed_groups));
    }

    Ok((extra, scopes, allowed_contracts, allowed_groups))
}
