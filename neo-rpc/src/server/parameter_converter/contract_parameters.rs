//! Contract-parameter array conversion.

use neo_execution::contract_parameter::ContractParameter;
use neo_serialization::json::JToken;

use super::{ConversionContext, RpcConvertible, expect_array, invalid_params, jtoken_to_serde};
use crate::server::rpc_exception::RpcException;

impl RpcConvertible for Vec<ContractParameter> {
    fn from_token(token: &JToken, _ctx: &ConversionContext) -> Result<Self, RpcException> {
        let array = expect_array(token)?;
        let mut parameters = Self::with_capacity(array.count());
        for (index, item) in array.children().iter().enumerate() {
            let token = item.as_ref().ok_or_else(|| {
                invalid_params(format!("Invalid contract parameter at index {index}"))
            })?;
            let value = jtoken_to_serde(token);
            let parameter = ContractParameter::from_json(&value).map_err(|e| {
                invalid_params(format!("Invalid contract parameter at index {index}: {e}"))
            })?;
            parameters.push(parameter);
        }
        Ok(parameters)
    }
}
