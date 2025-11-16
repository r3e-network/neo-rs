use bs58;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use neo_core::uint160::UInt160;

use super::rpc_server::RpcServer;

impl RpcServer {
    pub fn list_plugins(&self) -> Value {
        let infos = self
            .system()
            .plugin_manager()
            .read()
            .map(|manager| manager.plugin_infos())
            .unwrap_or_default();

        let items: Vec<Value> = infos
            .into_iter()
            .map(|(name, version)| {
                json!({
                    "name": name,
                    "version": version,
                    "interfaces": Vec::<Value>::new(),
                })
            })
            .collect();

        Value::Array(items)
    }

    pub fn validate_address(&self, address: &str) -> Value {
        let address = address.trim();
        let address_version = self.system().settings().address_version;
        let is_valid = parse_address_with_version(address, address_version)
            .or_else(|_| parse_address_with_version(address, 0x35))
            .is_ok();

        json!({
            "address": address,
            "isvalid": is_valid,
        })
    }
}

fn parse_address_with_version(address: &str, version: u8) -> Result<UInt160, ()> {
    let mut result = None;
    if UInt160::try_parse(address, &mut result) {
        if let Some(value) = result {
            return Ok(value);
        }
    }

    let decoded = bs58::decode(address).into_vec().map_err(|_| ())?;
    if decoded.len() != 25 {
        return Err(());
    }

    if decoded[0] != version {
        return Err(());
    }

    let data = &decoded[..21];
    let checksum = &decoded[21..];
    let first_hash = Sha256::digest(data);
    let second_hash = Sha256::digest(first_hash);
    if checksum != &second_hash[..4] {
        return Err(());
    }

    UInt160::from_bytes(&decoded[1..21]).map_err(|_| ())
}
