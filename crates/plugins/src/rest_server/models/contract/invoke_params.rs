// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Models.Contract.InvokeParams.

use neo_core::network::p2p::payloads::Signer;
use neo_core::smart_contract::contract_parameter::ContractParameter;
#[derive(Debug, Clone, Default)]
pub struct InvokeParams {
    pub contract_parameters: Vec<ContractParameter>,
    pub signers: Vec<Signer>,
}

impl InvokeParams {
    pub fn new(
        contract_parameters: Vec<ContractParameter>,
        signers: Vec<Signer>,
    ) -> Self {
        Self {
            contract_parameters,
            signers,
        }
    }
}
