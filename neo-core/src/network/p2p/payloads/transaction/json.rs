//
// json.rs - Transaction JSON serialization
//

use super::*;

impl Transaction {
    /// Converts the transaction to a JSON object.
    pub fn to_json(&self, settings: &ProtocolSettings) -> serde_json::Value {
        let mut json = serde_json::Map::new();

        json.insert(
            "hash".to_string(),
            serde_json::json!(self.hash().to_string()),
        );
        json.insert("size".to_string(), serde_json::json!(self.size()));
        json.insert("version".to_string(), serde_json::json!(self.version));
        json.insert("nonce".to_string(), serde_json::json!(self.nonce));

        let sender_value = self
            .sender()
            .map(|account| WalletHelper::to_address(&account, settings.address_version));
        json.insert(
            "sender".to_string(),
            sender_value
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );

        json.insert(
            "sysfee".to_string(),
            serde_json::json!(self.system_fee.to_string()),
        );
        json.insert(
            "netfee".to_string(),
            serde_json::json!(self.network_fee.to_string()),
        );
        json.insert(
            "validuntilblock".to_string(),
            serde_json::json!(self.valid_until_block),
        );

        let signers_json: Vec<_> = self.signers.iter().map(|s| s.to_json()).collect();
        json.insert(
            "signers".to_string(),
            serde_json::Value::Array(signers_json),
        );

        let attributes_json: Vec<_> = self.attributes.iter().map(|a| a.to_json()).collect();
        json.insert(
            "attributes".to_string(),
            serde_json::Value::Array(attributes_json),
        );

        json.insert(
            "script".to_string(),
            serde_json::json!(general_purpose::STANDARD.encode(&self.script)),
        );

        let witnesses_json: Vec<_> = self.witnesses.iter().map(|w| w.to_json()).collect();
        json.insert(
            "witnesses".to_string(),
            serde_json::Value::Array(witnesses_json),
        );

        serde_json::Value::Object(json)
    }
}
