/*
Package invoker provides a convenient wrapper to perform test calls via RPC client.

This layer builds on top of the basic RPC client and simplifies performing
test function invocations and script runs. It also makes historic calls (NeoGo
extension) transparent, allowing to use the same API as for regular calls.
Results of these calls can be interpreted by upper layer packages like actor
(to create transactions) or unwrap (to retrieve data from return values).
*/

use std::error::Error;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use crate::core::transaction::{self, Signer, Witness};
use crate::neorpc::result::{self, Invoke, Iterator};
use crate::smartcontract::{self, Parameter};
use crate::util::{self, Uint160, Uint256};
use crate::vm::stackitem::Item;

const DEFAULT_ITERATOR_RESULT_ITEMS: usize = 100;

pub trait RPCSessions {
    fn terminate_session(&self, session_id: Uuid) -> Result<bool, Box<dyn Error>>;
    fn traverse_iterator(&self, session_id: Uuid, iterator_id: Uuid, max_items_count: usize) -> Result<Vec<Item>, Box<dyn Error>>;
}

pub trait RPCInvoke: RPCSessions {
    fn invoke_contract_verify(&self, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_function(&self, contract: Uint160, operation: &str, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_script(&self, script: Vec<u8>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>>;
}

pub trait RPCInvokeHistoric: RPCSessions {
    fn invoke_contract_verify_at_height(&self, height: u32, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_contract_verify_with_state(&self, stateroot: Uint256, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_function_at_height(&self, height: u32, contract: Uint160, operation: &str, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_function_with_state(&self, stateroot: Uint256, contract: Uint160, operation: &str, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_script_at_height(&self, height: u32, script: Vec<u8>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>>;
    fn invoke_script_with_state(&self, stateroot: Uint256, script: Vec<u8>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>>;
}

pub struct Invoker {
    client: Arc<dyn RPCInvoke>,
    signers: Vec<Signer>,
}

struct HistoricConverter {
    client: Arc<dyn RPCInvokeHistoric>,
    height: Option<u32>,
    root: Option<Uint256>,
}

impl Invoker {
    pub fn new(client: Arc<dyn RPCInvoke>, signers: Vec<Signer>) -> Self {
        Self { client, signers }
    }

    pub fn new_historic_at_height(height: u32, client: Arc<dyn RPCInvokeHistoric>, signers: Vec<Signer>) -> Self {
        Self::new(Arc::new(HistoricConverter {
            client,
            height: Some(height),
            root: None,
        }), signers)
    }

    pub fn new_historic_with_state(root_or_block: Uint256, client: Arc<dyn RPCInvokeHistoric>, signers: Vec<Signer>) -> Self {
        Self::new(Arc::new(HistoricConverter {
            client,
            height: None,
            root: Some(root_or_block),
        }), signers)
    }

    pub fn signers(&self) -> Vec<Signer> {
        self.signers.clone()
    }

    pub fn call(&self, contract: Uint160, operation: &str, params: Vec<Parameter>) -> Result<Invoke, Box<dyn Error>> {
        self.client.invoke_function(contract, operation, params, self.signers.clone())
    }

    pub fn call_and_expand_iterator(&self, contract: Uint160, method: &str, max_items: usize, params: Vec<Parameter>) -> Result<Invoke, Box<dyn Error>> {
        let bytes = smartcontract::create_call_and_unwrap_iterator_script(contract, method, max_items, params)?;
        self.run(bytes)
    }

    pub fn verify(&self, contract: Uint160, witnesses: Vec<Witness>, params: Vec<Parameter>) -> Result<Invoke, Box<dyn Error>> {
        self.client.invoke_contract_verify(contract, params, self.signers.clone(), witnesses)
    }

    pub fn run(&self, script: Vec<u8>) -> Result<Invoke, Box<dyn Error>> {
        self.client.invoke_script(script, self.signers.clone())
    }

    pub fn terminate_session(&self, session_id: Uuid) -> Result<(), Box<dyn Error>> {
        term_session(self.client.as_ref(), session_id)
    }

    pub fn traverse_iterator(&self, session_id: Uuid, iterator: &Iterator, num: usize) -> Result<Vec<Item>, Box<dyn Error>> {
        iterate_next(self.client.as_ref(), session_id, iterator, num)
    }
}

impl HistoricConverter {
    fn invoke_script(&self, script: Vec<u8>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>> {
        if let Some(height) = self.height {
            self.client.invoke_script_at_height(height, script, signers)
        } else if let Some(root) = self.root {
            self.client.invoke_script_with_state(root, script, signers)
        } else {
            panic!("uninitialized historicConverter")
        }
    }

    fn invoke_function(&self, contract: Uint160, operation: &str, params: Vec<Parameter>, signers: Vec<Signer>) -> Result<Invoke, Box<dyn Error>> {
        if let Some(height) = self.height {
            self.client.invoke_function_at_height(height, contract, operation, params, signers)
        } else if let Some(root) = self.root {
            self.client.invoke_function_with_state(root, contract, operation, params, signers)
        } else {
            panic!("uninitialized historicConverter")
        }
    }

    fn invoke_contract_verify(&self, contract: Uint160, params: Vec<Parameter>, signers: Vec<Signer>, witnesses: Vec<Witness>) -> Result<Invoke, Box<dyn Error>> {
        if let Some(height) = self.height {
            self.client.invoke_contract_verify_at_height(height, contract, params, signers, witnesses)
        } else if let Some(root) = self.root {
            self.client.invoke_contract_verify_with_state(root, contract, params, signers, witnesses)
        } else {
            panic!("uninitialized historicConverter")
        }
    }

    fn terminate_session(&self, session_id: Uuid) -> Result<bool, Box<dyn Error>> {
        self.client.terminate_session(session_id)
    }

    fn traverse_iterator(&self, session_id: Uuid, iterator_id: Uuid, max_items_count: usize) -> Result<Vec<Item>, Box<dyn Error>> {
        self.client.traverse_iterator(session_id, iterator_id, max_items_count)
    }
}

fn term_session(rpc: &dyn RPCSessions, session_id: Uuid) -> Result<(), Box<dyn Error>> {
    let r = rpc.terminate_session(session_id)?;
    if !r {
        return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "terminate_session returned false")));
    }
    Ok(())
}

fn iterate_next(rpc: &dyn RPCSessions, session_id: Uuid, iterator: &Iterator, num: usize) -> Result<Vec<Item>, Box<dyn Error>> {
    let num = if num <= 0 { DEFAULT_ITERATOR_RESULT_ITEMS } else { num };
    if let Some(iterator_id) = &iterator.id {
        rpc.traverse_iterator(session_id, *iterator_id, num)
    } else {
        let num = std::cmp::min(num, iterator.values.len());
        let items = iterator.values[..num].to_vec();
        iterator.values = iterator.values[num..].to_vec();
        Ok(items)
    }
}
