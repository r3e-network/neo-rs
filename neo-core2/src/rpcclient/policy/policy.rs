/*
Package policy allows to work with the native PolicyContract contract via RPC.

Safe methods are encapsulated into ContractReader structure while Contract provides
various methods to perform PolicyContract state-changing calls.
*/

use crate::core::native::nativehashes;
use crate::core::transaction;
use crate::neorpc::result;
use crate::rpcclient::unwrap;
use crate::smartcontract;
use crate::util;
use std::error::Error;

// Invoker is used by ContractReader to call various methods.
pub trait Invoker {
    fn call(&self, contract: util::Uint160, operation: &str, params: &[impl Into<smartcontract::Parameter>]) -> Result<result::Invoke, Box<dyn Error>>;
}

// Actor is used by Contract to create and send transactions.
pub trait Actor: Invoker {
    fn make_call(&self, contract: util::Uint160, method: &str, params: &[impl Into<smartcontract::Parameter>]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_run(&self, script: &[u8]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: &[transaction::Attribute], params: &[impl Into<smartcontract::Parameter>]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_unsigned_run(&self, script: &[u8], attrs: &[transaction::Attribute]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: &[impl Into<smartcontract::Parameter>]) -> Result<(util::Uint256, u32), Box<dyn Error>>;
    fn send_run(&self, script: &[u8]) -> Result<(util::Uint256, u32), Box<dyn Error>>;
}

// Hash stores the hash of the native PolicyContract contract.
pub const HASH: util::Uint160 = nativehashes::POLICY_CONTRACT;

const EXEC_FEE_SETTER: &str = "setExecFeeFactor";
const FEE_PER_BYTE_SETTER: &str = "setFeePerByte";
const STORAGE_PRICE_SETTER: &str = "setStoragePrice";
const ATTRIBUTE_FEE_SETTER: &str = "setAttributeFee";

// ContractReader provides an interface to call read-only PolicyContract
// contract's methods.
pub struct ContractReader<I: Invoker> {
    invoker: I,
}

// Contract represents a PolicyContract contract client that can be used to
// invoke all of its methods.
pub struct Contract<A: Actor> {
    reader: ContractReader<A>,
    actor: A,
}

// NewReader creates an instance of ContractReader that can be used to read
// data from the contract.
impl<I: Invoker> ContractReader<I> {
    pub fn new(invoker: I) -> Self {
        Self { invoker }
    }

    // GetExecFeeFactor returns current execution fee factor used by the network.
    // This setting affects all executions of all transactions.
    pub fn get_exec_fee_factor(&self) -> Result<i64, Box<dyn Error>> {
        unwrap::int64(self.invoker.call(HASH, "getExecFeeFactor", &[]))
    }

    // GetFeePerByte returns current minimal per-byte network fee value which
    // affects all transactions on the network.
    pub fn get_fee_per_byte(&self) -> Result<i64, Box<dyn Error>> {
        unwrap::int64(self.invoker.call(HASH, "getFeePerByte", &[]))
    }

    // GetStoragePrice returns current per-byte storage price. Any contract saving
    // data to the storage pays for it according to this value.
    pub fn get_storage_price(&self) -> Result<i64, Box<dyn Error>> {
        unwrap::int64(self.invoker.call(HASH, "getStoragePrice", &[]))
    }

    // GetAttributeFee returns current fee for the specified attribute usage. Any
    // contract saving data to the storage pays for it according to this value.
    pub fn get_attribute_fee(&self, t: transaction::AttrType) -> Result<i64, Box<dyn Error>> {
        unwrap::int64(self.invoker.call(HASH, "getAttributeFee", &[t as u8]))
    }

    // IsBlocked checks if the given account is blocked in the PolicyContract.
    pub fn is_blocked(&self, account: util::Uint160) -> Result<bool, Box<dyn Error>> {
        unwrap::bool(self.invoker.call(HASH, "isBlocked", &[account]))
    }
}

// New creates an instance of Contract to perform actions using
// the given Actor. Notice that PolicyContract's state can be changed
// only by the network's committee, so the Actor provided must be a committee
// actor for all methods to work properly.
impl<A: Actor> Contract<A> {
    pub fn new(actor: A) -> Self {
        Self {
            reader: ContractReader::new(actor.clone()),
            actor,
        }
    }

    // SetExecFeeFactor creates and sends a transaction that sets the new
    // execution fee factor for the network to use. The action is successful when
    // transaction ends in HALT state. The returned values are transaction hash, its
    // ValidUntilBlock value and an error if any.
    pub fn set_exec_fee_factor(&self, value: i64) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, EXEC_FEE_SETTER, &[value])
    }

    // SetExecFeeFactorTransaction creates a transaction that sets the new execution
    // fee factor. This transaction is signed, but not sent to the network,
    // instead it's returned to the caller.
    pub fn set_exec_fee_factor_transaction(&self, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, EXEC_FEE_SETTER, &[value])
    }

    // SetExecFeeFactorUnsigned creates a transaction that sets the new execution
    // fee factor. This transaction is not signed and just returned to the caller.
    pub fn set_exec_fee_factor_unsigned(&self, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, EXEC_FEE_SETTER, &[], &[value])
    }

    // SetFeePerByte creates and sends a transaction that sets the new minimal
    // per-byte network fee value. The action is successful when transaction ends in
    // HALT state. The returned values are transaction hash, its ValidUntilBlock
    // value and an error if any.
    pub fn set_fee_per_byte(&self, value: i64) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, FEE_PER_BYTE_SETTER, &[value])
    }

    // SetFeePerByteTransaction creates a transaction that sets the new minimal
    // per-byte network fee value. This transaction is signed, but not sent to the
    // network, instead it's returned to the caller.
    pub fn set_fee_per_byte_transaction(&self, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, FEE_PER_BYTE_SETTER, &[value])
    }

    // SetFeePerByteUnsigned creates a transaction that sets the new minimal per-byte
    // network fee value. This transaction is not signed and just returned to the
    // caller.
    pub fn set_fee_per_byte_unsigned(&self, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, FEE_PER_BYTE_SETTER, &[], &[value])
    }

    // SetStoragePrice creates and sends a transaction that sets the storage price
    // for contracts. The action is successful when transaction ends in HALT
    // state. The returned values are transaction hash, its ValidUntilBlock value
    // and an error if any.
    pub fn set_storage_price(&self, value: i64) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, STORAGE_PRICE_SETTER, &[value])
    }

    // SetStoragePriceTransaction creates a transaction that sets the storage price
    // for contracts. This transaction is signed, but not sent to the network,
    // instead it's returned to the caller.
    pub fn set_storage_price_transaction(&self, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, STORAGE_PRICE_SETTER, &[value])
    }

    // SetStoragePriceUnsigned creates a transaction that sets the storage price
    // for contracts. This transaction is not signed and just returned to the
    // caller.
    pub fn set_storage_price_unsigned(&self, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, STORAGE_PRICE_SETTER, &[], &[value])
    }

    // SetAttributeFee creates and sends a transaction that sets the new attribute
    // fee value for the specified attribute. The action is successful when
    // transaction ends in HALT state. The returned values are transaction hash, its
    // ValidUntilBlock value and an error if any.
    pub fn set_attribute_fee(&self, t: transaction::AttrType, value: i64) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, ATTRIBUTE_FEE_SETTER, &[t as u8, value])
    }

    // SetAttributeFeeTransaction creates a transaction that sets the new attribute
    // fee value for the specified attribute. This transaction is signed, but not
    // sent to the network, instead it's returned to the caller.
    pub fn set_attribute_fee_transaction(&self, t: transaction::AttrType, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, ATTRIBUTE_FEE_SETTER, &[t as u8, value])
    }

    // SetAttributeFeeUnsigned creates a transaction that sets the new attribute fee
    // value for the specified attribute. This transaction is not signed and just
    // returned to the caller.
    pub fn set_attribute_fee_unsigned(&self, t: transaction::AttrType, value: i64) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, ATTRIBUTE_FEE_SETTER, &[], &[t as u8, value])
    }

    // BlockAccount creates and sends a transaction that blocks an account on the
    // network (via `blockAccount` method), it fails (with FAULT state) if it's not
    // successful. The returned values are transaction hash, its
    // ValidUntilBlock value and an error if any.
    pub fn block_account(&self, account: util::Uint160) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_run(&block_script(account))
    }

    // BlockAccountTransaction creates a transaction that blocks an account on the
    // network and checks for the result of the appropriate call, failing the
    // transaction if it's not true. This transaction is signed, but not sent to the
    // network, instead it's returned to the caller.
    pub fn block_account_transaction(&self, account: util::Uint160) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_run(&block_script(account))
    }

    // BlockAccountUnsigned creates a transaction that blocks an account on the
    // network and checks for the result of the appropriate call, failing the
    // transaction if it's not true. This transaction is not signed and just returned
    // to the caller.
    pub fn block_account_unsigned(&self, account: util::Uint160) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_run(&block_script(account), &[])
    }

    // UnblockAccount creates and sends a transaction that removes previously blocked
    // account from the stop list. It uses `unblockAccount` method and checks for the
    // result returned, failing the transaction if it's not true. The returned values
    // are transaction hash, its ValidUntilBlock value and an error if any.
    pub fn unblock_account(&self, account: util::Uint160) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_run(&unblock_script(account))
    }

    // UnblockAccountTransaction creates a transaction that unblocks previously
    // blocked account via `unblockAccount` method and checks for the result returned,
    // failing the transaction if it's not true. This transaction is signed, but not
    // sent to the network, instead it's returned to the caller.
    pub fn unblock_account_transaction(&self, account: util::Uint160) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_run(&unblock_script(account))
    }

    // UnblockAccountUnsigned creates a transaction that unblocks the given account
    // if it was blocked previously. It uses `unblockAccount` method and checks for
    // its return value, failing the transaction if it's not true. This transaction
    // is not signed and just returned to the caller.
    pub fn unblock_account_unsigned(&self, account: util::Uint160) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_run(&unblock_script(account), &[])
    }
}

fn block_script(account: util::Uint160) -> Vec<u8> {
    // We know parameters exactly (unlike with nep17.Transfer), so this can't fail.
    let script = smartcontract::create_call_with_assert_script(HASH, "blockAccount", &[account]).unwrap();
    script
}

fn unblock_script(account: util::Uint160) -> Vec<u8> {
    // We know parameters exactly (unlike with nep17.Transfer), so this can't fail.
    let script = smartcontract::create_call_with_assert_script(HASH, "unblockAccount", &[account]).unwrap();
    script
}
