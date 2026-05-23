//! OracleRequest - matches C# Neo.SmartContract.Native.OracleRequest exactly

use crate::error::CoreError;
use crate::{UInt160, UInt256};
use neo_vm_rs::StackValue;

/// Oracle request state (matches C# OracleRequest)
#[derive(Clone, Debug)]
pub struct OracleRequest {
    /// The original transaction hash
    pub original_tx_id: UInt256,

    /// The gas for callback
    pub gas_for_response: i64,

    /// The URL to fetch
    pub url: String,

    /// The filter expression
    pub filter: Option<String>,

    /// The callback contract hash
    pub callback_contract: UInt160,

    /// The callback method name
    pub callback_method: String,

    /// User data to pass to callback
    pub user_data: Vec<u8>,
}

impl OracleRequest {
    /// Creates a new oracle request
    pub fn new(
        original_tx_id: UInt256,
        gas_for_response: i64,
        url: String,
        filter: Option<String>,
        callback_contract: UInt160,
        callback_method: String,
        user_data: Vec<u8>,
    ) -> Self {
        Self {
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract,
            callback_method,
            user_data,
        }
    }

    /// Converts to a neo-vm-rs stack value.
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::ByteString(self.original_tx_id.to_bytes()),
            StackValue::Integer(self.gas_for_response),
            StackValue::ByteString(self.url.as_bytes().to_vec()),
            match &self.filter {
                Some(filter) => StackValue::ByteString(filter.as_bytes().to_vec()),
                None => StackValue::Null,
            },
            StackValue::ByteString(self.callback_contract.to_bytes()),
            StackValue::ByteString(self.callback_method.as_bytes().to_vec()),
            StackValue::ByteString(self.user_data.clone()),
        ])
    }

    /// Updates this oracle request from a neo-vm-rs stack value.
    pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Array(items) = stack_value {
            if items.len() < 7 {
                return Ok(());
            }

            if let Some(bytes) = items[0].to_byte_string_bytes() {
                if bytes.len() == 32 {
                    if let Ok(hash) = UInt256::from_bytes(&bytes) {
                        self.original_tx_id = hash;
                    }
                }
            }

            if let Some(value) = items[1]
                .to_i128()
                .and_then(|integer| i64::try_from(integer).ok())
            {
                self.gas_for_response = value;
            }

            if let Some(bytes) = items[2].to_byte_string_bytes() {
                if let Ok(url) = String::from_utf8(bytes) {
                    self.url = url;
                }
            }

            if matches!(items[3], StackValue::Null) {
                self.filter = None;
            } else if let Some(bytes) = items[3].to_byte_string_bytes() {
                if let Ok(filter) = String::from_utf8(bytes) {
                    self.filter = Some(filter);
                }
            }

            if let Some(bytes) = items[4].to_byte_string_bytes() {
                if bytes.len() == 20 {
                    if let Ok(hash) = UInt160::from_bytes(&bytes) {
                        self.callback_contract = hash;
                    }
                }
            }

            if let Some(bytes) = items[5].to_byte_string_bytes() {
                if let Ok(method) = String::from_utf8(bytes) {
                    self.callback_method = method;
                }
            }

            if let Some(bytes) = items[6].to_byte_string_bytes() {
                self.user_data = bytes;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    fn sample_request(filter: Option<&str>) -> OracleRequest {
        OracleRequest::new(
            UInt256::from_bytes(&[1u8; 32]).unwrap(),
            1_000,
            "https://oracle.test/data".to_string(),
            filter.map(str::to_string),
            UInt160::from_bytes(&[2u8; 20]).unwrap(),
            "callback".to_string(),
            vec![3, 4, 5],
        )
    }

    #[test]
    fn oracle_request_projects_to_neo_vm_rs_stack_value() {
        let request = sample_request(Some("$.price"));

        assert_eq!(
            request.to_stack_value(),
            StackValue::Array(vec![
                StackValue::ByteString(vec![1u8; 32]),
                StackValue::Integer(1_000),
                StackValue::ByteString(b"https://oracle.test/data".to_vec()),
                StackValue::ByteString(b"$.price".to_vec()),
                StackValue::ByteString(vec![2u8; 20]),
                StackValue::ByteString(b"callback".to_vec()),
                StackValue::ByteString(vec![3, 4, 5]),
            ])
        );
    }

    #[test]
    fn oracle_request_reads_from_neo_vm_rs_stack_value() {
        let mut request = sample_request(Some("old"));

        request
            .from_stack_value(StackValue::Array(vec![
                StackValue::ByteString(vec![9u8; 32]),
                StackValue::Integer(99),
                StackValue::ByteString(b"https://new.test".to_vec()),
                StackValue::Null,
                StackValue::ByteString(vec![8u8; 20]),
                StackValue::ByteString(b"updated".to_vec()),
                StackValue::ByteString(vec![7, 6]),
            ]))
            .unwrap();

        assert_eq!(
            request.original_tx_id,
            UInt256::from_bytes(&[9u8; 32]).unwrap()
        );
        assert_eq!(request.gas_for_response, 99);
        assert_eq!(request.url, "https://new.test");
        assert_eq!(request.filter, None);
        assert_eq!(
            request.callback_contract,
            UInt160::from_bytes(&[8u8; 20]).unwrap()
        );
        assert_eq!(request.callback_method, "updated");
        assert_eq!(request.user_data, vec![7, 6]);
    }
}
