//! Address, block, and contract identifier RPC conversions.

use neo_serialization::json::JToken;

use super::{
    ConversionContext, RpcConvertible, expect_array, expect_string, invalid_params, parse_address,
};
use crate::server::model::{Address, BlockHashOrIndex, ContractNameOrHashOrId};
use crate::server::rpc_exception::RpcException;

impl RpcConvertible for Address {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let text = expect_string(token, "Expected address string")?;
        parse_address(&text, ctx.address_version)
    }
}

impl RpcConvertible for Vec<Address> {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        let mut result = Self::with_capacity(array.count());
        for (index, item) in array.children().iter().enumerate() {
            let token = item
                .as_ref()
                .ok_or_else(|| invalid_params(format!("Null address entry at index {index}")))?;
            result.push(<Address as RpcConvertible>::from_token(token, ctx)?);
        }
        Ok(result)
    }
}

impl RpcConvertible for BlockHashOrIndex {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        match token {
            JToken::Number(value) => {
                if value.is_nan() || value.is_infinite() {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                let rounded = value.round();
                if (value - rounded).abs() > f64::EPSILON {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                if rounded < 0.0 || rounded > f64::from(u32::MAX) {
                    return Err(invalid_params(format!(
                        "Invalid block index value: {}",
                        token.to_string_value()
                    )));
                }
                Ok(Self::from_index(rounded as u32))
            }
            JToken::String(text) => Self::try_parse(text)
                .ok_or_else(|| invalid_params(format!("Invalid block hash or index: {text}"))),
            _ => Err(invalid_params("Expected block hash or index string")),
        }
    }
}

impl RpcConvertible for ContractNameOrHashOrId {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        match token {
            JToken::Number(value) => {
                if value.is_nan() || value.is_infinite() {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                let rounded = value.round();
                if (value - rounded).abs() > f64::EPSILON {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
                    return Err(invalid_params(format!(
                        "Invalid contract identifier: {}",
                        token.to_string_value()
                    )));
                }
                Ok(Self::from_id(rounded as i32))
            }
            JToken::String(text) => Self::try_parse(text)
                .ok_or_else(|| invalid_params(format!("Invalid contract identifier: {text}"))),
            _ => Err(invalid_params("Expected contract identifier string")),
        }
    }
}
