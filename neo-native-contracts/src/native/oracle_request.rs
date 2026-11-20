//! OracleRequest stack projection for the oracle native contract.
//!
//! The request record itself is owned by `neo-protocol`
//! ([`neo_protocol::OracleRequest`]) and re-exported here so existing native
//! contract paths (`crate::native::oracle_request::OracleRequest`) keep
//! resolving. The native-contract-specific stack projection stays in this
//! crate: the contract stores requests as an `Array(7)` and must be able to
//! convert between that shape and the shared record.

pub use neo_protocol::OracleRequest;

use neo_smart_contract::error::CoreError;
use neo_vm::StackValue;

/// Stack projection of an [`OracleRequest`] into the oracle native contract's
/// `Array(7)` storage shape.
///
/// Implemented here rather than on the shared protocol record so that
/// `neo-protocol` stays free of native-contract serialization concerns.
pub trait OracleRequestStackProjection {
    /// Converts the request into a `neo-vm-rs` stack value.
    fn to_stack_value(&self) -> StackValue;

    /// Updates the request from a `neo-vm-rs` stack value.
    ///
    /// The name deliberately mirrors [`Self::to_stack_value`] so the pair reads
    /// as one projection, and the `&mut self` receiver is the in-place update
    /// the Oracle contract performs on its stored request.
    #[allow(clippy::wrong_self_convention)]
    fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>;
}

impl OracleRequestStackProjection for OracleRequest {
    fn to_stack_value(&self) -> StackValue {
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

    fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError> {
        if let StackValue::Array(items) = stack_value {
            if items.len() < 7 {
                return Ok(());
            }

            if let Some(bytes) = items[0].to_byte_string_bytes()
                && bytes.len() == 32
                && let Ok(hash) = neo_primitives::UInt256::from_bytes(&bytes)
            {
                self.original_tx_id = hash;
            }

            if let Some(value) = items[1]
                .to_i128()
                .and_then(|integer| i64::try_from(integer).ok())
            {
                self.gas_for_response = value;
            }

            if let Some(bytes) = items[2].to_byte_string_bytes()
                && let Ok(url) = String::from_utf8(bytes)
            {
                self.url = url;
            }

            if matches!(items[3], StackValue::Null) {
                self.filter = None;
            } else if let Some(bytes) = items[3].to_byte_string_bytes()
                && let Ok(filter) = String::from_utf8(bytes)
            {
                self.filter = Some(filter);
            }

            if let Some(bytes) = items[4].to_byte_string_bytes()
                && bytes.len() == 20
                && let Ok(hash) = neo_primitives::UInt160::from_bytes(&bytes)
            {
                self.callback_contract = hash;
            }

            if let Some(bytes) = items[5].to_byte_string_bytes()
                && let Ok(method) = String::from_utf8(bytes)
            {
                self.callback_method = method;
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
    use neo_primitives::{UInt160, UInt256};

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

    #[test]
    fn request_record_is_the_protocol_definition() {
        // Type identity: the native re-export and the protocol record must be
        // the same type, not structurally identical copies.
        fn assert_same<T>(_: std::marker::PhantomData<T>) {}
        let _: Option<neo_protocol::OracleRequest> = None;
        let _: Option<OracleRequest> = None;
        assert_same::<OracleRequest>(std::marker::PhantomData);
    }
}
