use serde::{Deserialize, Serialize};
use serde_json::{self, Error};
use std::error::Error as StdError;

#[derive(Serialize, Deserialize)]
pub struct TransactionOutputRaw {
    #[serde(flatten)]
    transaction: transaction::Transaction,
    #[serde(flatten)]
    metadata: TransactionMetadata,
}

#[derive(Serialize, Deserialize)]
pub struct TransactionMetadata {
    #[serde(rename = "blockhash", skip_serializing_if = "Option::is_none")]
    blockhash: Option<util::Uint256>,
    #[serde(rename = "confirmations", skip_serializing_if = "Option::is_none")]
    confirmations: Option<i32>,
    #[serde(rename = "blocktime", skip_serializing_if = "Option::is_none")]
    timestamp: Option<u64>,
    #[serde(rename = "vmstate", skip_serializing_if = "Option::is_none")]
    vmstate: Option<String>,
}

impl TransactionOutputRaw {
    pub fn marshal_json(&self) -> Result<Vec<u8>, Box<dyn StdError>> {
        let mut output = serde_json::to_vec(&self.metadata)?;
        let tx_bytes = serde_json::to_vec(&self.transaction)?;

        if output.last() != Some(&b'}') || tx_bytes.first() != Some(&b'{') {
            return Err("can't merge internal jsons".into());
        }

        output.pop();
        output.push(b',');
        output.extend_from_slice(&tx_bytes[1..]);
        Ok(output)
    }

    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        let metadata: TransactionMetadata = serde_json::from_slice(data)?;
        self.metadata = metadata;
        self.transaction = serde_json::from_slice(data)?;
        Ok(())
    }
}
