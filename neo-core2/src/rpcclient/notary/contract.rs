/*
Package notary provides an RPC-based wrapper for the Notary subsystem.

It provides both regular ContractReader/Contract interfaces for the notary
contract and notary-specific Actor as well as some helper functions to simplify
creation of notary requests.
*/

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::convert::TryInto;
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;

use crate::core::native::nativehashes;
use crate::core::transaction;
use crate::neorpc::result;
use crate::rpcclient::unwrap;
use crate::smartcontract;
use crate::util;
use crate::vm::stackitem;

const SET_MAX_NVB_DELTA_METHOD: &str = "setMaxNotValidBeforeDelta";
const SET_FEE_PK_METHOD: &str = "setNotaryServiceFeePerKey";

// ContractInvoker is used by ContractReader to perform read-only calls.
pub trait ContractInvoker {
    fn call(&self, contract: util::Uint160, operation: &str, params: &[stackitem::Item]) -> Result<result::Invoke, Box<dyn Error>>;
}

// ContractActor is used by Contract to create and send transactions.
pub trait ContractActor: ContractInvoker {
    fn make_call(&self, contract: util::Uint160, method: &str, params: &[stackitem::Item]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_run(&self, script: &[u8]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: &[transaction::Attribute], params: &[stackitem::Item]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_unsigned_run(&self, script: &[u8], attrs: &[transaction::Attribute]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: &[stackitem::Item]) -> Result<(util::Uint256, u32), Box<dyn Error>>;
    fn send_run(&self, script: &[u8]) -> Result<(util::Uint256, u32), Box<dyn Error>>;
}

// ContractReader represents safe (read-only) methods of Notary. It can be
// used to query various data, but `verify` method is not exposed there because
// it can't be successful in standalone invocation (missing transaction with the
// NotaryAssisted attribute and its signature).
pub struct ContractReader {
    invoker: Arc<dyn ContractInvoker>,
}

// Contract provides full Notary interface, both safe and state-changing methods.
// The only method omitted is onNEP17Payment which can only be called
// successfully from the GASToken native contract.
pub struct Contract {
    reader: ContractReader,
    actor: Arc<dyn ContractActor>,
}

// OnNEP17PaymentData is the data set that is accepted by the notary contract
// onNEP17Payment handler. It's mandatory for GAS tranfers to this contract.
pub struct OnNEP17PaymentData {
    // Account can be None, in this case transfer sender (from) account is used.
    account: Option<util::Uint160>,
    // Till specifies the deposit lock time (in blocks).
    till: u32,
}

// OnNEP17PaymentData have to implement stackitem::Convertible interface to be
// compatible with emit package.
impl stackitem::Convertible for OnNEP17PaymentData {
    fn to_stack_item(&self) -> Result<stackitem::Item, Box<dyn Error>> {
        Ok(stackitem::Item::Array(vec![
            stackitem::Item::from(self.account.clone()),
            stackitem::Item::from(self.till),
        ]))
    }

    fn from_stack_item(item: stackitem::Item) -> Result<Self, Box<dyn Error>> {
        let arr = item.as_array()?;
        if arr.len() != 2 {
            return Err(Box::new(fmt::Error::new(fmt::Error, "unexpected number of fields")));
        }

        let account = if arr[0] != stackitem::Item::Null {
            Some(util::Uint160::from_bytes_be(&arr[0].as_bytes()?))
        } else {
            None
        };

        let till = arr[1].as_integer()?.to_u32().ok_or_else(|| fmt::Error::new(fmt::Error, "till is not a u32"))?;

        Ok(OnNEP17PaymentData { account, till })
    }
}

// Hash stores the hash of the native Notary contract.
pub static HASH: util::Uint160 = nativehashes::NOTARY;

// NewReader creates an instance of ContractReader to get data from the Notary
// contract.
pub fn new_reader(invoker: Arc<dyn ContractInvoker>) -> ContractReader {
    ContractReader { invoker }
}

// New creates an instance of Contract to perform state-changing actions in the
// Notary contract.
pub fn new(actor: Arc<dyn ContractActor>) -> Contract {
    Contract {
        reader: new_reader(actor.clone()),
        actor,
    }
}

// BalanceOf returns the locked GAS balance for the given account.
impl ContractReader {
    pub fn balance_of(&self, account: util::Uint160) -> Result<BigInt, Box<dyn Error>> {
        unwrap::big_int(self.invoker.call(HASH, "balanceOf", &[stackitem::Item::from(account)]))
    }

    // ExpirationOf returns the index of the block when the GAS deposit for the given
    // account will expire.
    pub fn expiration_of(&self, account: util::Uint160) -> Result<u32, Box<dyn Error>> {
        let res = self.invoker.call(HASH, "expirationOf", &[stackitem::Item::from(account)])?;
        let ret = unwrap::limited_int64(res, 0, u32::MAX as i64)?;
        Ok(ret as u32)
    }

    // GetMaxNotValidBeforeDelta returns the maximum NotValidBefore attribute delta
    // that can be used in notary-assisted transactions.
    pub fn get_max_not_valid_before_delta(&self) -> Result<u32, Box<dyn Error>> {
        let res = self.invoker.call(HASH, "getMaxNotValidBeforeDelta", &[])?;
        let ret = unwrap::limited_int64(res, 0, u32::MAX as i64)?;
        Ok(ret as u32)
    }
}

// LockDepositUntil creates and sends a transaction that extends the deposit lock
// time for the given account. The return result from the "lockDepositUntil"
// method is checked to be true, so transaction fails (with FAULT state) if not
// successful. The returned values are transaction hash, its ValidUntilBlock
// value and an error if any.
impl Contract {
    pub fn lock_deposit_until(&self, account: util::Uint160, index: u32) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_run(&lock_script(account, index))
    }

    // LockDepositUntilTransaction creates a transaction that extends the deposit lock
    // time for the given account. The return result from the "lockDepositUntil"
    // method is checked to be true, so transaction fails (with FAULT state) if not
    // successful. The returned values are transaction hash, its ValidUntilBlock
    // value and an error if any. The transaction is signed, but not sent to the
    // network, instead it's returned to the caller.
    pub fn lock_deposit_until_transaction(&self, account: util::Uint160, index: u32) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_run(&lock_script(account, index))
    }

    // LockDepositUntilUnsigned creates a transaction that extends the deposit lock
    // time for the given account. The return result from the "lockDepositUntil"
    // method is checked to be true, so transaction fails (with FAULT state) if not
    // successful. The returned values are transaction hash, its ValidUntilBlock
    // value and an error if any. The transaction is not signed and just returned to
    // the caller.
    pub fn lock_deposit_until_unsigned(&self, account: util::Uint160, index: u32) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_run(&lock_script(account, index), &[])
    }
}

fn lock_script(account: util::Uint160, index: u32) -> Vec<u8> {
    // We know parameters exactly (unlike with nep17.Transfer), so this can't fail.
    smartcontract::create_call_with_assert_script(HASH, "lockDepositUntil", &[stackitem::Item::from(account.to_bytes_be()), stackitem::Item::from(index as i64)]).unwrap()
}

// SetMaxNotValidBeforeDelta creates and sends a transaction that sets the new
// maximum NotValidBefore attribute value delta that can be used in
// notary-assisted transactions. The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The returned values are
// transaction hash, its ValidUntilBlock value and an error if any.
impl Contract {
    pub fn set_max_not_valid_before_delta(&self, blocks: u32) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, SET_MAX_NVB_DELTA_METHOD, &[stackitem::Item::from(blocks)])
    }

    // SetMaxNotValidBeforeDeltaTransaction creates a transaction that sets the new
    // maximum NotValidBefore attribute value delta that can be used in
    // notary-assisted transactions. The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The transaction is signed,
    // but not sent to the network, instead it's returned to the caller.
    pub fn set_max_not_valid_before_delta_transaction(&self, blocks: u32) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, SET_MAX_NVB_DELTA_METHOD, &[stackitem::Item::from(blocks)])
    }

    // SetMaxNotValidBeforeDeltaUnsigned creates a transaction that sets the new
    // maximum NotValidBefore attribute value delta that can be used in
    // notary-assisted transactions. The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The transaction is not
    // signed and just returned to the caller.
    pub fn set_max_not_valid_before_delta_unsigned(&self, blocks: u32) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, SET_MAX_NVB_DELTA_METHOD, &[], &[stackitem::Item::from(blocks)])
    }

    // Withdraw creates and sends a transaction that withdraws the deposit belonging
    // to "from" account and sends it to "to" account. The return result from the
    // "withdraw" method is checked to be true, so transaction fails (with FAULT
    // state) if not successful. The returned values are transaction hash, its
    // ValidUntilBlock value and an error if any.
    pub fn withdraw(&self, from: util::Uint160, to: util::Uint160) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_run(&withdraw_script(from, to))
    }

    // WithdrawTransaction creates a transaction that withdraws the deposit belonging
    // to "from" account and sends it to "to" account. The return result from the
    // "withdraw" method is checked to be true, so transaction fails (with FAULT
    // state) if not successful. The transaction is signed, but not sent to the
    // network, instead it's returned to the caller.
    pub fn withdraw_transaction(&self, from: util::Uint160, to: util::Uint160) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_run(&withdraw_script(from, to))
    }

    // WithdrawUnsigned creates a transaction that withdraws the deposit belonging
    // to "from" account and sends it to "to" account. The return result from the
    // "withdraw" method is checked to be true, so transaction fails (with FAULT
    // state) if not successful. The transaction is not signed and just returned to
    // the caller.
    pub fn withdraw_unsigned(&self, from: util::Uint160, to: util::Uint160) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_run(&withdraw_script(from, to), &[])
    }
}

fn withdraw_script(from: util::Uint160, to: util::Uint160) -> Vec<u8> {
    // We know parameters exactly (unlike with nep17.Transfer), so this can't fail.
    smartcontract::create_call_with_assert_script(HASH, "withdraw", &[stackitem::Item::from(from.to_bytes_be()), stackitem::Item::from(to.to_bytes_be())]).unwrap()
}
