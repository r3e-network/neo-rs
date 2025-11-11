use alloc::string::String;

use serde_json::Value;

pub(super) struct SignerReader<'a> {
    pub(super) account: &'a str,
    pub(super) scopes: &'a str,
    pub(super) value: &'a Value,
}

impl<'a> SignerReader<'a> {
    pub fn new(value: &'a Value) -> Result<Self, String> {
        let obj = value
            .as_object()
            .ok_or_else(|| "Signer JSON must be an object".to_string())?;

        let account = obj
            .get("account")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Signer.account must be a string".to_string())?;

        let scopes = obj
            .get("scopes")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Signer.scopes must be a string".to_string())?;

        Ok(Self {
            account,
            scopes,
            value,
        })
    }
}
