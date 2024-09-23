/*
Package neorpc contains a set of types used for JSON-RPC communication with Neo servers.
It defines basic request/response types as well as a set of errors and additional
parameters used for specific requests/responses.
*/

use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::fmt;
use std::str::FromStr;

use crate::core::transaction;
use crate::crypto::keys;
use crate::encoding::address;
use crate::util;

pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Serialize, Deserialize)]
pub struct Request {
    // JSONRPC is the protocol version, only valid when it contains JSONRPCVersion.
    pub jsonrpc: String,
    // Method is the method being called.
    pub method: String,
    // Params is a set of method-specific parameters passed to the call. They
    // can be anything as long as they can be marshaled to JSON correctly and
    // used by the method implementation on the server side. While JSON-RPC
    // technically allows it to be an object, all Neo calls expect params
    // to be an array.
    pub params: Vec<Value>,
    // ID is an identifier associated with this request. JSON-RPC itself allows
    // any strings to be used for it as well, but NeoGo RPC client uses numeric
    // identifiers.
    pub id: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Header {
    pub id: Value,
    pub jsonrpc: String,
}

#[derive(Serialize, Deserialize)]
pub struct HeaderAndError {
    pub header: Header,
    pub error: Option<Error>,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub header_and_error: HeaderAndError,
    pub result: Option<Value>,
}

#[derive(Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: EventID,
    pub params: Vec<Value>,
}

#[derive(Serialize, Deserialize)]
pub struct SignerWithWitness {
    pub signer: transaction::Signer,
    pub witness: transaction::Witness,
}

#[derive(Serialize, Deserialize)]
struct SignerWithWitnessAux {
    account: String,
    scopes: Value,
    allowedcontracts: Option<Vec<util::Uint160>>,
    allowedgroups: Option<Vec<keys::PublicKey>>,
    rules: Option<Vec<transaction::WitnessRule>>,
    invocation: Option<Vec<u8>>,
    verification: Option<Vec<u8>>,
}

impl Serialize for SignerWithWitness {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let scopes = self.signer.scopes.to_string();
        let aux = SignerWithWitnessAux {
            account: format!("0x{}", self.signer.account.to_string()),
            scopes: Value::String(scopes),
            allowedcontracts: Some(self.signer.allowed_contracts.clone()),
            allowedgroups: Some(self.signer.allowed_groups.clone()),
            rules: Some(self.signer.rules.clone()),
            invocation: Some(self.witness.invocation_script.clone()),
            verification: Some(self.witness.verification_script.clone()),
        };
        aux.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SignerWithWitness {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let aux = SignerWithWitnessAux::deserialize(deserializer)?;
        let account = util::Uint160::from_str(&aux.account.trim_start_matches("0x"))
            .map_err(serde::de::Error::custom)?;
        let scopes = transaction::WitnessScope::from_str(aux.scopes.as_str().unwrap())
            .map_err(serde::de::Error::custom)?;
        Ok(SignerWithWitness {
            signer: transaction::Signer {
                account,
                scopes,
                allowed_contracts: aux.allowedcontracts.unwrap_or_default(),
                allowed_groups: aux.allowedgroups.unwrap_or_default(),
                rules: aux.rules.unwrap_or_default(),
            },
            witness: transaction::Witness {
                invocation_script: aux.invocation.unwrap_or_default(),
                verification_script: aux.verification.unwrap_or_default(),
            },
        })
    }
}

impl Notification {
    pub fn event_id(&self) -> EventID {
        self.method
    }

    pub fn event_payload(&self) -> &Value {
        &self.params[0]
    }
}
