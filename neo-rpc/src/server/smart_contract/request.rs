//! Typed request parsing for smart-contract invocation RPC handlers.
//!
//! `invokefunction` and `invokescript` have positional JSON parameters with
//! C# binder-compatible signer, witness, and diagnostic defaults. Keeping that
//! decoding here lets the invocation handler focus on script construction,
//! execution, and result projection.

use neo_execution::contract_parameter::ContractParameter;
use neo_payloads::signer::Signer;
use neo_payloads::witness::Witness;
use neo_primitives::UInt160;
use serde_json::Value;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::expect_base64_param_with_decode_message;
use crate::server::rpc_server::RpcServer;

use super::helpers::{
    expect_script_hash_param, expect_string_param, parse_contract_parameters,
    parse_signers_and_witnesses,
};

pub(super) struct InvokeFunctionRequest {
    pub(super) script_hash: UInt160,
    pub(super) operation: String,
    pub(super) parameters: Vec<ContractParameter>,
    pub(super) signers: Option<Vec<Signer>>,
    pub(super) witnesses: Option<Vec<Witness>>,
    pub(super) use_diagnostic: bool,
}

impl InvokeFunctionRequest {
    pub(super) fn parse(server: &RpcServer, params: &[Value]) -> Result<Self, RpcException> {
        let script_hash = expect_script_hash_param(params, 0, "invokefunction")?;
        let operation = expect_string_param(params, 1, "invokefunction")?;
        let parameters = parse_contract_parameters(params.get(2))?;
        let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(3))?;
        Ok(Self {
            script_hash,
            operation,
            parameters,
            signers,
            witnesses,
            use_diagnostic: parse_diagnostic_flag(params.get(4)),
        })
    }
}

pub(super) struct InvokeScriptRequest {
    pub(super) script: Vec<u8>,
    pub(super) signers: Option<Vec<Signer>>,
    pub(super) witnesses: Option<Vec<Witness>>,
    pub(super) use_diagnostic: bool,
}

impl InvokeScriptRequest {
    pub(super) fn parse(server: &RpcServer, params: &[Value]) -> Result<Self, RpcException> {
        let script = expect_base64_param_with_decode_message(
            params,
            0,
            "invokescript",
            "invalid script payload",
        )?;
        let (signers, witnesses) = parse_signers_and_witnesses(server, params.get(1))?;
        Ok(Self {
            script,
            signers,
            witnesses,
            use_diagnostic: parse_diagnostic_flag(params.get(2)),
        })
    }
}

fn parse_diagnostic_flag(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}
