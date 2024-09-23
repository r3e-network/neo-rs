/*
Package oracle allows to work with the native OracleContract contract via RPC.

Safe methods are encapsulated into ContractReader structure while Contract provides
various methods to perform state-changing calls.
*/

use std::sync::Arc;
use std::error::Error;
use std::convert::TryInto;
use num_bigint::BigInt;
use crate::core::native::nativehashes;
use crate::core::transaction;
use crate::neorpc::result;
use crate::rpcclient::unwrap;
use crate::util;
use crate::vm::stackitem;

// Invoker is used by ContractReader to call various methods.
pub trait Invoker {
    fn call(&self, contract: util::Uint160, operation: &str, params: &[stackitem::Item]) -> Result<result::Invoke, Box<dyn Error>>;
}

// Actor is used by Contract to create and send transactions.
pub trait Actor: Invoker {
    fn make_call(&self, contract: util::Uint160, method: &str, params: &[stackitem::Item]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: &[transaction::Attribute], params: &[stackitem::Item]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: &[stackitem::Item]) -> Result<(util::Uint256, u32), Box<dyn Error>>;
}

// Hash stores the hash of the native OracleContract contract.
pub const HASH: util::Uint160 = nativehashes::OracleContract;

const PRICE_SETTER: &str = "setPrice";

// ContractReader provides an interface to call read-only OracleContract
// contract's methods. "verify" method is not exposed since it's very specific
// and can't be executed successfully outside of the proper oracle response
// transaction.
pub struct ContractReader {
    invoker: Arc<dyn Invoker>,
}

// Contract represents the OracleContract contract client that can be used to
// invoke its "setPrice" method. Other methods are useless for direct calls,
// "request" requires a callback that entry script can't provide and "finish"
// will only work in an oracle transaction. Since "setPrice" can be called
// successfully only by the network's committee, an appropriate Actor is needed
// for Contract.
pub struct Contract {
    reader: ContractReader,
    actor: Arc<dyn Actor>,
}

// RequestEvent represents an OracleRequest notification event emitted from
// the OracleContract contract.
pub struct RequestEvent {
    pub id: i64,
    pub contract: util::Uint160,
    pub url: String,
    pub filter: String,
}

// ResponseEvent represents an OracleResponse notification event emitted from
// the OracleContract contract.
pub struct ResponseEvent {
    pub id: i64,
    pub original_tx: util::Uint256,
}

// NewReader creates an instance of ContractReader that can be used to read
// data from the contract.
pub fn new_reader(invoker: Arc<dyn Invoker>) -> ContractReader {
    ContractReader { invoker }
}

// New creates an instance of Contract to perform actions using
// the given Actor.
pub fn new(actor: Arc<dyn Actor>) -> Contract {
    Contract {
        reader: new_reader(actor.clone()),
        actor,
    }
}

// GetPrice returns current price of the oracle request call.
impl ContractReader {
    pub fn get_price(&self) -> Result<BigInt, Box<dyn Error>> {
        unwrap::big_int(self.invoker.call(HASH, "getPrice", &[]))
    }
}

// SetPrice creates and sends a transaction that sets the new price for the
// oracle request call. The action is successful when transaction ends in HALT
// state. The returned values are transaction hash, its ValidUntilBlock value and
// an error if any.
impl Contract {
    pub fn set_price(&self, value: &BigInt) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, PRICE_SETTER, &[stackitem::Item::from(value.clone())])
    }

    // SetPriceTransaction creates a transaction that sets the new price for the
    // oracle request call. The action is successful when transaction ends in HALT
    // state. The transaction is signed, but not sent to the network, instead it's
    // returned to the caller.
    pub fn set_price_transaction(&self, value: &BigInt) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, PRICE_SETTER, &[stackitem::Item::from(value.clone())])
    }

    // SetPriceUnsigned creates a transaction that sets the new price for the
    // oracle request call. The action is successful when transaction ends in HALT
    // state. The transaction is not signed and just returned to the caller.
    pub fn set_price_unsigned(&self, value: &BigInt) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, PRICE_SETTER, &[], &[stackitem::Item::from(value.clone())])
    }
}
