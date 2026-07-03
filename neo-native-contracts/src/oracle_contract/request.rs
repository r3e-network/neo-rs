//! Oracle request storage value types.

use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// A pending oracle request (mirrors C# `OracleRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OracleRequest {
    /// The original transaction hash that created the request.
    pub original_tx_id: UInt256,
    /// GAS allocated for the response.
    pub gas_for_response: i64,
    /// The URL to fetch.
    pub url: String,
    /// Optional JSONPath filter.
    pub filter: Option<String>,
    /// Callback contract hash.
    pub callback_contract: UInt160,
    /// Callback method name.
    pub callback_method: String,
    /// User data (opaque payload, BinarySerializer-encoded).
    pub user_data: Vec<u8>,
}

impl OracleRequest {
    /// Construct a new oracle request (used by tests and by the
    /// service when emitting transactions).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        original_tx_id: UInt256,
        gas_for_response: i64,
        url: impl Into<String>,
        filter: Option<String>,
        callback_contract: UInt160,
        callback_method: impl Into<String>,
        user_data: Vec<u8>,
    ) -> Self {
        Self {
            original_tx_id,
            gas_for_response,
            url: url.into(),
            filter,
            callback_contract,
            callback_method: callback_method.into(),
            user_data,
        }
    }

    /// Converts to the C# `OracleRequest.ToStackItem` layout.
    pub fn to_stack_value(&self) -> StackValue {
        let filter_item = match &self.filter {
            Some(filter) => StackValue::ByteString(filter.as_bytes().to_vec()),
            None => StackValue::Null,
        };
        StackValue::Array(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(self.original_tx_id.to_bytes()),
                StackValue::Integer(self.gas_for_response),
                StackValue::ByteString(self.url.as_bytes().to_vec()),
                filter_item,
                StackValue::ByteString(self.callback_contract.to_bytes()),
                StackValue::ByteString(self.callback_method.as_bytes().to_vec()),
                StackValue::ByteString(self.user_data.clone()),
            ],
        )
    }

    /// Parses the C# `OracleRequest.FromStackItem` layout from a StackValue.
    pub fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(_, items) = stack_value else {
            return Err(CoreError::invalid_data("OracleRequest is not an array"));
        };
        if items.len() != 7 {
            return Err(CoreError::invalid_data("OracleRequest must have 7 fields"));
        }
        let field_bytes = |index: usize, name: &str| -> CoreResult<Vec<u8>> {
            items[index]
                .to_byte_string_bytes()
                .ok_or_else(|| CoreError::invalid_data(format!("OracleRequest {name}: not bytes")))
        };
        let field_string = |index: usize, name: &str| -> CoreResult<String> {
            String::from_utf8(field_bytes(index, name)?)
                .map_err(|e| CoreError::invalid_data(format!("OracleRequest {name}: {e}")))
        };
        let original_tx_id = UInt256::from_bytes(&field_bytes(0, "OriginalTxid")?)
            .map_err(|e| CoreError::invalid_data(format!("OracleRequest OriginalTxid: {e}")))?;
        let gas_for_response = neo_vm::stack_value_as_bigint(&items[1])
            .map_err(|e| CoreError::invalid_data(format!("OracleRequest GasForResponse: {e}")))?
            .to_i64()
            .ok_or_else(|| CoreError::invalid_data("OracleRequest GasForResponse out of range"))?;
        let url = field_string(2, "Url")?;
        let filter = if matches!(items[3], StackValue::Null) {
            None
        } else {
            Some(field_string(3, "Filter")?)
        };
        let callback_contract = crate::args::bytes_to_hash160(
            &field_bytes(4, "CallbackContract")?,
            "OracleRequest CallbackContract",
        )?;
        let callback_method = field_string(5, "CallbackMethod")?;
        let user_data = field_bytes(6, "UserData")?;
        Ok(Self {
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract,
            callback_method,
            user_data,
        })
    }
}

neo_vm::impl_interoperable_via_stack_value!(OracleRequest);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OracleIdList {
    ids: Vec<u64>,
}

impl OracleIdList {
    pub(super) fn new(ids: Vec<u64>) -> Self {
        Self { ids }
    }

    pub(super) fn into_ids(self) -> Vec<u64> {
        self.ids
    }

    pub(super) fn to_stack_value(&self) -> StackValue {
        StackValue::Array(
            neo_vm_rs::next_stack_item_id(),
            self.ids
                .iter()
                .map(|id| StackValue::BigInteger(BigInt::from(*id).to_signed_bytes_le()))
                .collect(),
        )
    }

    pub(super) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(_, items) = stack_value else {
            return Err(CoreError::invalid_data("Oracle IdList is not an array"));
        };
        let mut ids = Vec::with_capacity(items.len());
        for entry in &items {
            let id = neo_vm::stack_value_as_bigint(entry)
                .map_err(|e| CoreError::invalid_data(format!("Oracle IdList entry: {e}")))?
                .to_u64()
                .ok_or_else(|| CoreError::invalid_data("Oracle IdList entry out of range"))?;
            ids.push(id);
        }
        Ok(Self { ids })
    }
}

neo_vm::impl_interoperable_via_stack_value!(OracleIdList);
