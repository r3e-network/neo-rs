use neo_execution::contract_parameter::ContractParameter;
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;
use neo_primitives::UInt160;
use neo_serialization::json::JToken;
use serde_json::Value;
use uuid::Uuid;

use crate::server::model::signers_and_witnesses::SignersAndWitnesses;
use crate::server::parameter_converter::{ConversionContext, ParameterConverter};
use crate::server::rpc_exception::RpcException;
pub(super) use crate::server::rpc_helpers::{
    expect_string_param, expect_u32_param, expect_uint160_param_with_message, internal_error,
    invalid_params,
};
use crate::server::rpc_server::RpcServer;

pub(super) fn parse_contract_parameters(
    arg: Option<&Value>,
) -> Result<Vec<ContractParameter>, RpcException> {
    match arg {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                ContractParameter::from_json(value).map_err(|e| invalid_params(e.to_string()))
            })
            .collect(),
        Some(_) => Err(invalid_params("args must be an array")),
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn parse_signers_and_witnesses(
    server: &RpcServer,
    value: Option<&Value>,
) -> Result<(Option<Vec<Signer>>, Option<Vec<Witness>>), RpcException> {
    let Some(token_value) = value else {
        return Ok((None, None));
    };
    let jtoken: JToken = serde_json::from_value(token_value.clone())
        .map_err(|err| invalid_params(err.to_string()))?;
    let ctx = ConversionContext::new(server.system().settings().address_version);
    let parsed = ParameterConverter::convert::<SignersAndWitnesses>(&jtoken, &ctx)?;
    let signers = if parsed.signers().is_empty() {
        None
    } else {
        Some(parsed.signers().to_vec())
    };
    let witnesses = if parsed.witnesses().is_empty() {
        None
    } else {
        Some(parsed.witnesses().to_vec())
    };
    Ok((signers, witnesses))
}

pub(super) fn expect_script_hash_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<UInt160, RpcException> {
    expect_uint160_param_with_message(
        params,
        index,
        format!("{} expects string parameter {}", method, index + 1),
        "script hash",
    )
}

pub(super) fn expect_uuid_param(
    params: &[Value],
    index: usize,
    method: &str,
) -> Result<Uuid, RpcException> {
    let text = expect_string_param(params, index, method)?;
    Uuid::parse_str(text.trim())
        .map_err(|_| invalid_params(format!("{} expects GUID parameter {}", method, index + 1)))
}
