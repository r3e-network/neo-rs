/*
Package management provides an RPC wrapper for the native ContractManagement contract.

Safe methods are encapsulated in the ContractReader structure while Contract provides
various methods to perform state-changing calls.
*/

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::fmt;

use uuid::Uuid;
use serde::{Serialize, Deserialize};
use serde_json;
use bigdecimal::BigDecimal;

use crate::core::native::nativehashes;
use crate::core::state;
use crate::core::transaction;
use crate::neorpc::result;
use crate::rpcclient::unwrap;
use crate::smartcontract;
use crate::smartcontract::manifest;
use crate::smartcontract::nef;
use crate::util;
use crate::vm::stackitem;

// Invoker is used by ContractReader to call various methods.
pub trait Invoker {
    fn call(&self, contract: util::Uint160, operation: &str, params: Vec<stackitem::Item>) -> Result<result::Invoke, Box<dyn std::error::Error>>;
    fn call_and_expand_iterator(&self, contract: util::Uint160, method: &str, max_items: i32, params: Vec<stackitem::Item>) -> Result<result::Invoke, Box<dyn std::error::Error>>;
    fn terminate_session(&self, session_id: Uuid) -> Result<(), Box<dyn std::error::Error>>;
    fn traverse_iterator(&self, session_id: Uuid, iterator: &result::Iterator, num: i32) -> Result<Vec<stackitem::Item>, Box<dyn std::error::Error>>;
}

// Actor is used by Contract to create and send transactions.
pub trait Actor: Invoker {
    fn make_call(&self, contract: util::Uint160, method: &str, params: Vec<stackitem::Item>) -> Result<transaction::Transaction, Box<dyn std::error::Error>>;
    fn make_run(&self, script: Vec<u8>) -> Result<transaction::Transaction, Box<dyn std::error::Error>>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: Vec<transaction::Attribute>, params: Vec<stackitem::Item>) -> Result<transaction::Transaction, Box<dyn std::error::Error>>;
    fn make_unsigned_run(&self, script: Vec<u8>, attrs: Vec<transaction::Attribute>) -> Result<transaction::Transaction, Box<dyn std::error::Error>>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: Vec<stackitem::Item>) -> Result<(util::Uint256, u32), Box<dyn std::error::Error>>;
    fn send_run(&self, script: Vec<u8>) -> Result<(util::Uint256, u32), Box<dyn std::error::Error>>;
}

// ContractReader provides an interface to call read-only ContractManagement
// contract's methods.
pub struct ContractReader {
    invoker: Arc<dyn Invoker>,
}

// Contract represents a ContractManagement contract client that can be used to
// invoke all of its methods except 'update' and 'destroy' because they can be
// called successfully only from the contract itself (that is doing an update
// or self-destruction).
pub struct Contract {
    contract_reader: ContractReader,
    actor: Arc<dyn Actor>,
}

// IDHash is an ID/Hash pair returned by the iterator from the GetContractHashes method.
#[derive(Serialize, Deserialize)]
pub struct IDHash {
    id: i32,
    hash: util::Uint160,
}

// HashesIterator is used for iterating over GetContractHashes results.
pub struct HashesIterator {
    client: Arc<dyn Invoker>,
    session: Uuid,
    iterator: result::Iterator,
}

// Hash stores the hash of the native ContractManagement contract.
pub static HASH: util::Uint160 = nativehashes::CONTRACT_MANAGEMENT;

// Event is the event emitted on contract deployment/update/destroy.
// Even though these events are different they all have the same field inside.
#[derive(Serialize, Deserialize)]
pub struct Event {
    hash: util::Uint160,
}

const SET_MIN_FEE_METHOD: &str = "setMinimumDeploymentFee";

// NewReader creates an instance of ContractReader that can be used to read
// data from the contract.
pub fn new_reader(invoker: Arc<dyn Invoker>) -> ContractReader {
    ContractReader { invoker }
}

// New creates an instance of Contract to perform actions using
// the given Actor.
pub fn new(actor: Arc<dyn Actor>) -> Contract {
    Contract {
        contract_reader: new_reader(actor.clone()),
        actor,
    }
}

// GetContract allows to get contract data from its hash. This method is mostly
// useful for historic invocations since for current contracts there is a direct
// getcontractstate RPC API that has more options and works faster than going
// via contract invocation.
impl ContractReader {
    pub fn get_contract(&self, hash: util::Uint160) -> Result<state::Contract, Box<dyn std::error::Error>> {
        unwrap_contract(self.invoker.call(HASH, "getContract", vec![stackitem::Item::from(hash)]))
    }

    // GetContractByID allows to get contract data from its ID. In case of missing
    // contract it returns nil state.Contract and nil error.
    pub fn get_contract_by_id(&self, id: i32) -> Result<state::Contract, Box<dyn std::error::Error>> {
        unwrap_contract(self.invoker.call(HASH, "getContractById", vec![stackitem::Item::from(id)]))
    }

    // GetContractHashes returns an iterator that allows to retrieve all ID-hash
    // mappings for non-native contracts. It depends on the server to provide proper
    // session-based iterator, but can also work with expanded one.
    pub fn get_contract_hashes(&self) -> Result<HashesIterator, Box<dyn std::error::Error>> {
        let (sess, iter) = unwrap::session_iterator(self.invoker.call(HASH, "getContractHashes")?)?;
        Ok(HashesIterator {
            client: self.invoker.clone(),
            session: sess,
            iterator: iter,
        })
    }

    // GetContractHashesExpanded is similar to GetContractHashes (uses the same
    // contract method), but can be useful if the server used doesn't support
    // sessions and doesn't expand iterators. It creates a script that will get num
    // of result items from the iterator right in the VM and return them to you. It's
    // only limited by VM stack and GAS available for RPC invocations.
    pub fn get_contract_hashes_expanded(&self, num: i32) -> Result<Vec<IDHash>, Box<dyn std::error::Error>> {
        let arr = unwrap::array(self.invoker.call_and_expand_iterator(HASH, "getContractHashes", num, vec![]))?;
        items_to_id_hashes(arr)
    }

    // GetMinimumDeploymentFee returns the minimal amount of GAS needed to deploy a
    // contract on the network.
    pub fn get_minimum_deployment_fee(&self) -> Result<BigDecimal, Box<dyn std::error::Error>> {
        unwrap::big_int(self.invoker.call(HASH, "getMinimumDeploymentFee"))
    }

    // HasMethod checks if the contract specified has a method with the given name
    // and number of parameters.
    pub fn has_method(&self, hash: util::Uint160, method: &str, pcount: i32) -> Result<bool, Box<dyn std::error::Error>> {
        unwrap::bool(self.invoker.call(HASH, "hasMethod", vec![stackitem::Item::from(hash), stackitem::Item::from(method), stackitem::Item::from(pcount)]))
    }
}

// Next returns the next set of elements from the iterator (up to num of them).
// It can return less than num elements in case iterator doesn't have that many
// or zero elements if the iterator has no more elements or the session is
// expired.
impl HashesIterator {
    pub fn next(&self, num: i32) -> Result<Vec<IDHash>, Box<dyn std::error::Error>> {
        let items = self.client.traverse_iterator(self.session, &self.iterator, num)?;
        items_to_id_hashes(items)
    }

    // Terminate closes the iterator session used by HashesIterator (if it's
    // session-based).
    pub fn terminate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.iterator.id.is_none() {
            return Ok(());
        }
        self.client.terminate_session(self.session)
    }
}

fn items_to_id_hashes(arr: Vec<stackitem::Item>) -> Result<Vec<IDHash>, Box<dyn std::error::Error>> {
    let mut res = Vec::with_capacity(arr.len());
    for (i, itm) in arr.iter().enumerate() {
        let str = itm.value_as_array().ok_or_else(|| fmt::format(format_args!("item #{} is not a structure {:?}", i, itm.value())))?;
        if str.len() != 2 {
            return Err(fmt::format(format_args!("item #{} has wrong length", i)).into());
        }
        let bi = str[0].try_bytes()?;
        if bi.len() != 4 {
            return Err(fmt::format(format_args!("item #{} has wrong ID: bad length", i)).into());
        }
        let id = i32::from_be_bytes(bi.try_into().unwrap());
        let hb = str[1].try_bytes()?;
        let u160 = util::Uint160::decode_bytes_be(&hb)?;
        res.push(IDHash { id, hash: u160 });
    }
    Ok(res)
}

// unwrapContract tries to retrieve state.Contract from the provided result.Invoke.
// If the resulting stack contains stackitem.Null, then nil state and nil error
// will be returned.
fn unwrap_contract(r: Result<result::Invoke, Box<dyn std::error::Error>>) -> Result<state::Contract, Box<dyn std::error::Error>> {
    let itm = unwrap::item(r)?;
    if itm.equals(&stackitem::Item::Null) {
        return Ok(None);
    }
    let mut res = state::Contract::default();
    res.from_stack_item(&itm)?;
    Ok(Some(res))
}

// Deploy creates and sends to the network a transaction that deploys the given
// contract (with the manifest provided), if data is not nil then it also added
// to the invocation and will be used for "_deploy" method invocation done by
// the ContractManagement contract. If successful, this method returns deployed
// contract state that can be retrieved from the stack after execution.
impl Contract {
    pub fn deploy(&self, exe: &nef::File, manif: &manifest::Manifest, data: Option<stackitem::Item>) -> Result<(util::Uint256, u32), Box<dyn std::error::Error>> {
        let script = mk_deploy_script(exe, manif, data)?;
        self.actor.send_run(script)
    }

    // DeployTransaction creates and returns a transaction that deploys the given
    // contract (with the manifest provided), if data is not nil then it also added
    // to the invocation and will be used for "_deploy" method invocation done by
    // the ContractManagement contract. If successful, this method returns deployed
    // contract state that can be retrieved from the stack after execution.
    pub fn deploy_transaction(&self, exe: &nef::File, manif: &manifest::Manifest, data: Option<stackitem::Item>) -> Result<transaction::Transaction, Box<dyn std::error::Error>> {
        let script = mk_deploy_script(exe, manif, data)?;
        self.actor.make_run(script)
    }

    // DeployUnsigned creates and returns an unsigned transaction that deploys the given
    // contract (with the manifest provided), if data is not nil then it also added
    // to the invocation and will be used for "_deploy" method invocation done by
    // the ContractManagement contract. If successful, this method returns deployed
    // contract state that can be retrieved from the stack after execution.
    pub fn deploy_unsigned(&self, exe: &nef::File, manif: &manifest::Manifest, data: Option<stackitem::Item>) -> Result<transaction::Transaction, Box<dyn std::error::Error>> {
        let script = mk_deploy_script(exe, manif, data)?;
        self.actor.make_unsigned_run(script, vec![])
    }

    // SetMinimumDeploymentFee creates and sends a transaction that changes the
    // minimum GAS amount required to deploy a contract. This method can be called
    // successfully only by the network's committee, so make sure you're using an
    // appropriate Actor. This invocation returns nothing and is successful when
    // transactions ends up in the HALT state.
    pub fn set_minimum_deployment_fee(&self, value: &BigDecimal) -> Result<(util::Uint256, u32), Box<dyn std::error::Error>> {
        self.actor.send_call(HASH, SET_MIN_FEE_METHOD, vec![stackitem::Item::from(value)])
    }

    // SetMinimumDeploymentFeeTransaction creates a transaction that changes the
    // minimum GAS amount required to deploy a contract. This method can be called
    // successfully only by the network's committee, so make sure you're using an
    // appropriate Actor. This invocation returns nothing and is successful when
    // transactions ends up in the HALT state. The transaction returned is signed,
    // but not sent to the network.
    pub fn set_minimum_deployment_fee_transaction(&self, value: &BigDecimal) -> Result<transaction::Transaction, Box<dyn std::error::Error>> {
        self.actor.make_call(HASH, SET_MIN_FEE_METHOD, vec![stackitem::Item::from(value)])
    }

    // SetMinimumDeploymentFeeUnsigned creates a transaction that changes the
    // minimum GAS amount required to deploy a contract. This method can be called
    // successfully only by the network's committee, so make sure you're using an
    // appropriate Actor. This invocation returns nothing and is successful when
    // transactions ends up in the HALT state. The transaction returned is not
    // signed.
    pub fn set_minimum_deployment_fee_unsigned(&self, value: &BigDecimal) -> Result<transaction::Transaction, Box<dyn std::error::Error>> {
        self.actor.make_unsigned_call(HASH, SET_MIN_FEE_METHOD, vec![], vec![stackitem::Item::from(value)])
    }
}

fn mk_deploy_script(exe: &nef::File, manif: &manifest::Manifest, data: Option<stackitem::Item>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let exe_b = exe.bytes()?;
    let manif_b = serde_json::to_vec(manif)?;
    if let Some(data) = data {
        smartcontract::create_call_script(HASH, "deploy", vec![exe_b, manif_b, data])
    } else {
        smartcontract::create_call_script(HASH, "deploy", vec![exe_b, manif_b])
    }
}
