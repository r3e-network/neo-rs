/*
Package neo provides an RPC-based wrapper for the NEOToken contract.

Safe methods are encapsulated into ContractReader structure while Contract provides
various methods to perform state-changing calls.
*/
use std::fmt;
use std::sync::Arc;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;
use elliptic_curve::sec1::ToEncodedPoint;
use elliptic_curve::p256::NistP256;
use elliptic_curve::PublicKey;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::core::native::nativehashes;
use crate::core::state;
use crate::core::transaction;
use crate::crypto::keys;
use crate::neorpc::result;
use crate::rpcclient::nep17;
use crate::rpcclient::unwrap;
use crate::smartcontract;
use crate::util;
use crate::vm::stackitem;

const SET_GAS_METHOD: &str = "setGasPerBlock";
const SET_REG_METHOD: &str = "setRegisterPrice";

// Invoker is used by ContractReader to perform read-only calls.
pub trait Invoker: nep17::Invoker {
    fn call_and_expand_iterator(&self, contract: util::Uint160, method: &str, max_items: i32, params: Vec<stackitem::Item>) -> Result<result::Invoke, String>;
    fn terminate_session(&self, session_id: Uuid) -> Result<(), String>;
    fn traverse_iterator(&self, session_id: Uuid, iterator: &result::Iterator, num: i32) -> Result<Vec<stackitem::Item>, String>;
}

// Actor is used by Contract to create and send transactions.
pub trait Actor: nep17::Actor + Invoker {
    fn run(&self, script: Vec<u8>) -> Result<result::Invoke, String>;
    fn make_call(&self, contract: util::Uint160, method: &str, params: Vec<stackitem::Item>) -> Result<transaction::Transaction, String>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: Vec<transaction::Attribute>, params: Vec<stackitem::Item>) -> Result<transaction::Transaction, String>;
    fn make_unsigned_unchecked_run(&self, script: Vec<u8>, sys_fee: i64, attrs: Vec<transaction::Attribute>) -> Result<transaction::Transaction, String>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: Vec<stackitem::Item>) -> Result<(util::Uint256, u32), String>;
    fn sign(&self, tx: &mut transaction::Transaction) -> Result<(), String>;
    fn sign_and_send(&self, tx: &mut transaction::Transaction) -> Result<(util::Uint256, u32), String>;
}

// ContractReader represents safe (read-only) methods of NEO. It can be
// used to query various data.
pub struct ContractReader {
    token_reader: nep17::TokenReader,
    invoker: Arc<dyn Invoker>,
}

// Contract provides full NEO interface, both safe and state-changing methods.
pub struct Contract {
    contract_reader: ContractReader,
    token_writer: nep17::TokenWriter,
    actor: Arc<dyn Actor>,
}

// CandidateStateEvent represents a CandidateStateChanged NEO event.
pub struct CandidateStateEvent {
    key: keys::PublicKey,
    registered: bool,
    votes: BigInt,
}

// CommitteeChangedEvent represents a CommitteeChanged NEO event.
pub struct CommitteeChangedEvent {
    old: Vec<keys::PublicKey>,
    new: Vec<keys::PublicKey>,
}

// VoteEvent represents a Vote NEO event.
pub struct VoteEvent {
    account: util::Uint160,
    from: keys::PublicKey,
    to: keys::PublicKey,
    amount: BigInt,
}

// ValidatorIterator is used for iterating over GetAllCandidates results.
pub struct ValidatorIterator {
    client: Arc<dyn Invoker>,
    session: Uuid,
    iterator: result::Iterator,
}

// Hash stores the hash of the native NEOToken contract.
pub static HASH: util::Uint160 = nativehashes::NEO_TOKEN;

// NewReader creates an instance of ContractReader to get data from the NEO
// contract.
pub fn new_reader(invoker: Arc<dyn Invoker>) -> ContractReader {
    ContractReader {
        token_reader: nep17::new_reader(invoker.clone(), HASH),
        invoker,
    }
}

// New creates an instance of Contract to perform state-changing actions in the
// NEO contract.
pub fn new(actor: Arc<dyn Actor>) -> Contract {
    let nep = nep17::new(actor.clone(), HASH);
    Contract {
        contract_reader: ContractReader {
            token_reader: nep.token_reader,
            invoker: actor.clone(),
        },
        token_writer: nep.token_writer,
        actor,
    }
}

// GetAccountState returns current NEO balance state for the account which
// includes balance and voting data. It can return nil balance with no error
// if the account given has no NEO.
impl ContractReader {
    pub fn get_account_state(&self, account: util::Uint160) -> Result<Option<state::NEOBalance>, String> {
        let itm = unwrap::item(self.invoker.call(HASH, "getAccountState", vec![stackitem::Item::from(account)]))?;
        if itm.is_null() {
            return Ok(None);
        }
        let mut res = state::NEOBalance::default();
        res.from_stack_item(&itm)?;
        Ok(Some(res))
    }

    // GetAllCandidates returns an iterator that allows to retrieve all registered
    // validators from it. It depends on the server to provide proper session-based
    // iterator, but can also work with expanded one.
    pub fn get_all_candidates(&self) -> Result<ValidatorIterator, String> {
        let (sess, iter) = unwrap::session_iterator(self.invoker.call(HASH, "getAllCandidates", vec![]))?;
        Ok(ValidatorIterator {
            client: self.invoker.clone(),
            iterator: iter,
            session: sess,
        })
    }

    // GetAllCandidatesExpanded is similar to GetAllCandidates (uses the same NEO
    // method), but can be useful if the server used doesn't support sessions and
    // doesn't expand iterators. It creates a script that will get num of result
    // items from the iterator right in the VM and return them to you. It's only
    // limited by VM stack and GAS available for RPC invocations.
    pub fn get_all_candidates_expanded(&self, num: i32) -> Result<Vec<result::Validator>, String> {
        let arr = unwrap::array(self.invoker.call_and_expand_iterator(HASH, "getAllCandidates", num, vec![]))?;
        items_to_validators(arr)
    }

    // GetCandidates returns the list of validators with their vote count. This
    // method is mostly useful for historic invocations because the RPC protocol
    // provides direct getcandidates call that returns more data and works faster.
    // The contract only returns up to 256 candidates in response to this method, so
    // if there are more of them on the network you will get a truncated result, use
    // GetAllCandidates to solve this problem.
    pub fn get_candidates(&self) -> Result<Vec<result::Validator>, String> {
        let arr = unwrap::array(self.invoker.call(HASH, "getCandidates", vec![]))?;
        items_to_validators(arr)
    }

    // GetCommittee returns the list of committee member public keys. This
    // method is mostly useful for historic invocations because the RPC protocol
    // provides direct getcommittee call that works faster.
    pub fn get_committee(&self) -> Result<keys::PublicKeys, String> {
        unwrap::array_of_public_keys(self.invoker.call(HASH, "getCommittee", vec![]))
    }

    // GetCommitteeAddress returns the committee address.
    pub fn get_committee_address(&self) -> Result<util::Uint160, String> {
        unwrap::uint160(self.invoker.call(HASH, "getCommitteeAddress", vec![]))
    }

    // GetNextBlockValidators returns the list of validator keys that will sign the
    // next block. This method is mostly useful for historic invocations because the
    // RPC protocol provides direct getnextblockvalidators call that provides more
    // data and works faster.
    pub fn get_next_block_validators(&self) -> Result<keys::PublicKeys, String> {
        unwrap::array_of_public_keys(self.invoker.call(HASH, "getNextBlockValidators", vec![]))
    }

    // GetGasPerBlock returns the amount of GAS generated in each block.
    pub fn get_gas_per_block(&self) -> Result<i64, String> {
        unwrap::int64(self.invoker.call(HASH, "getGasPerBlock", vec![]))
    }

    // GetRegisterPrice returns the price of candidate key registration.
    pub fn get_register_price(&self) -> Result<i64, String> {
        unwrap::int64(self.invoker.call(HASH, "getRegisterPrice", vec![]))
    }

    // UnclaimedGas allows to calculate the amount of GAS that will be generated if
    // any NEO state change ("claim") is to happen for the given account at the given
    // block number. This method is mostly useful for historic invocations because
    // the RPC protocol provides direct getunclaimedgas method that works faster.
    pub fn unclaimed_gas(&self, account: util::Uint160, end: u32) -> Result<BigInt, String> {
        unwrap::big_int(self.invoker.call(HASH, "unclaimedGas", vec![stackitem::Item::from(account), stackitem::Item::from(end)]))
    }
}

impl ValidatorIterator {
    // Next returns the next set of elements from the iterator (up to num of them).
    // It can return less than num elements in case iterator doesn't have that many
    // or zero elements if the iterator has no more elements or the session is
    // expired.
    pub fn next(&self, num: i32) -> Result<Vec<result::Validator>, String> {
        let items = self.client.traverse_iterator(self.session, &self.iterator, num)?;
        items_to_validators(items)
    }

    // Terminate closes the iterator session used by ValidatorIterator (if it's
    // session-based).
    pub fn terminate(&self) -> Result<(), String> {
        if self.iterator.id.is_none() {
            return Ok(());
        }
        self.client.terminate_session(self.session)
    }
}

impl Contract {
    // RegisterCandidate creates and sends a transaction that adds the given key to
    // the list of candidates that can be voted for. The return result from the
    // "registerCandidate" method is checked to be true, so transaction fails (with
    // FAULT state) if not successful. Notice that for this call to work it must be
    // witnessed by the simple account derived from the given key, so use an
    // appropriate Actor. The returned values are transaction hash, its
    // ValidUntilBlock value and an error if any.
    //
    // Notice that unlike for all other methods the script for this one is not
    // test-executed in its final form because most networks have registration price
    // set to be much higher than typical RPC server allows to spend during
    // test-execution. This adds some risk that it might fail on-chain, but in
    // practice it's not likely to happen if signers are set up correctly.
    pub fn register_candidate(&self, k: &keys::PublicKey) -> Result<(util::Uint256, u32), String> {
        let mut tx = self.register_candidate_unsigned(k)?;
        self.actor.sign_and_send(&mut tx)
    }

    // RegisterCandidateTransaction creates a transaction that adds the given key to
    // the list of candidates that can be voted for. The return result from the
    // "registerCandidate" method is checked to be true, so transaction fails (with
    // FAULT state) if not successful. Notice that for this call to work it must be
    // witnessed by the simple account derived from the given key, so use an
    // appropriate Actor. The transaction is signed, but not sent to the network,
    // instead it's returned to the caller.
    //
    // Notice that unlike for all other methods the script for this one is not
    // test-executed in its final form because most networks have registration price
    // set to be much higher than typical RPC server allows to spend during
    // test-execution. This adds some risk that it might fail on-chain, but in
    // practice it's not likely to happen if signers are set up correctly.
    pub fn register_candidate_transaction(&self, k: &keys::PublicKey) -> Result<transaction::Transaction, String> {
        let mut tx = self.register_candidate_unsigned(k)?;
        self.actor.sign(&mut tx)?;
        Ok(tx)
    }

    // RegisterCandidateUnsigned creates a transaction that adds the given key to
    // the list of candidates that can be voted for. The return result from the
    // "registerCandidate" method is checked to be true, so transaction fails (with
    // FAULT state) if not successful. Notice that for this call to work it must be
    // witnessed by the simple account derived from the given key, so use an
    // appropriate Actor. The transaction is not signed and just returned to the
    // caller.
    //
    // Notice that unlike for all other methods the script for this one is not
    // test-executed in its final form because most networks have registration price
    // set to be much higher than typical RPC server allows to spend during
    // test-execution. This adds some risk that it might fail on-chain, but in
    // practice it's not likely to happen if signers are set up correctly.
    pub fn register_candidate_unsigned(&self, k: &keys::PublicKey) -> Result<transaction::Transaction, String> {
        // It's an unregister script intentionally.
        let r = self.actor.run(reg_script(true, k))?;
        let reg_price = self.contract_reader.get_register_price()?;
        self.actor.make_unsigned_unchecked_run(reg_script(false, k), r.gas_consumed + reg_price, vec![])
    }

    // UnregisterCandidate creates and sends a transaction that removes the key from
    // the list of candidates that can be voted for. The return result from the
    // "unregisterCandidate" method is checked to be true, so transaction fails (with
    // FAULT state) if not successful. Notice that for this call to work it must be
    // witnessed by the simple account derived from the given key, so use an
    // appropriate Actor. The returned values are transaction hash, its
    // ValidUntilBlock value and an error if any.
    pub fn unregister_candidate(&self, k: &keys::PublicKey) -> Result<(util::Uint256, u32), String> {
        self.actor.send_run(reg_script(true, k))
    }

    // UnregisterCandidateTransaction creates a transaction that removes the key from
    // the list of candidates that can be voted for. The return result from the
    // "unregisterCandidate" method is checked to be true, so transaction fails (with
    // FAULT state) if not successful. Notice that for this call to work it must be
    // witnessed by the simple account derived from the given key, so use an
    // appropriate Actor. The transaction is signed, but not sent to the network,
    // instead it's returned to the caller.
    pub fn unregister_candidate_transaction(&self, k: &keys::PublicKey) -> Result<transaction::Transaction, String> {
        self.actor.make_run(reg_script(true, k))
    }

    // UnregisterCandidateUnsigned creates a transaction that removes the key from
    // the list of candidates that can be voted for. The return result from the
    // "unregisterCandidate" method is checked to be true, so transaction fails (with
    // FAULT state) if not successful. Notice that for this call to work it must be
    // witnessed by the simple account derived from the given key, so use an
    // appropriate Actor. The transaction is not signed and just returned to the
    // caller.
    pub fn unregister_candidate_unsigned(&self, k: &keys::PublicKey) -> Result<transaction::Transaction, String> {
        self.actor.make_unsigned_run(reg_script(true, k), vec![])
    }

    // Vote creates and sends a transaction that casts a vote from the given account
    // to the given key which can be nil (in which case any previous vote is removed).
    // The return result from the "vote" method is checked to be true, so transaction
    // fails (with FAULT state) if voting is not successful. The returned values are
    // transaction hash, its ValidUntilBlock value and an error if any.
    pub fn vote(&self, account: util::Uint160, vote_to: Option<&keys::PublicKey>) -> Result<(util::Uint256, u32), String> {
        self.actor.send_run(vote_script(account, vote_to))
    }

    // VoteTransaction creates a transaction that casts a vote from the given account
    // to the given key which can be nil (in which case any previous vote is removed).
    // The return result from the "vote" method is checked to be true, so transaction
    // fails (with FAULT state) if voting is not successful. The transaction is signed,
    // but not sent to the network, instead it's returned to the caller.
    pub fn vote_transaction(&self, account: util::Uint160, vote_to: Option<&keys::PublicKey>) -> Result<transaction::Transaction, String> {
        self.actor.make_run(vote_script(account, vote_to))
    }

    // VoteUnsigned creates a transaction that casts a vote from the given account
    // to the given key which can be nil (in which case any previous vote is removed).
    // The return result from the "vote" method is checked to be true, so transaction
    // fails (with FAULT state) if voting is not successful. The transaction is not
    // signed and just returned to the caller.
    pub fn vote_unsigned(&self, account: util::Uint160, vote_to: Option<&keys::PublicKey>) -> Result<transaction::Transaction, String> {
        self.actor.make_unsigned_run(vote_script(account, vote_to), vec![])
    }

    // SetGasPerBlock creates and sends a transaction that sets the new amount of
    // GAS to be generated in each block. The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The returned values are
    // transaction hash, its ValidUntilBlock value and an error if any.
    pub fn set_gas_per_block(&self, gas: i64) -> Result<(util::Uint256, u32), String> {
        self.actor.send_call(HASH, SET_GAS_METHOD, vec![stackitem::Item::from(gas)])
    }

    // SetGasPerBlockTransaction creates a transaction that sets the new amount of
    // GAS to be generated in each block. The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The transaction is signed,
    // but not sent to the network, instead it's returned to the caller.
    pub fn set_gas_per_block_transaction(&self, gas: i64) -> Result<transaction::Transaction, String> {
        self.actor.make_call(HASH, SET_GAS_METHOD, vec![stackitem::Item::from(gas)])
    }

    // SetGasPerBlockUnsigned creates a transaction that sets the new amount of
    // GAS to be generated in each block. The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The transaction is not
    // signed and just returned to the caller.
    pub fn set_gas_per_block_unsigned(&self, gas: i64) -> Result<transaction::Transaction, String> {
        self.actor.make_unsigned_call(HASH, SET_GAS_METHOD, vec![], vec![stackitem::Item::from(gas)])
    }

    // SetRegisterPrice creates and sends a transaction that sets the new candidate
    // registration price (in GAS). The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The returned values are
    // transaction hash, its ValidUntilBlock value and an error if any.
    pub fn set_register_price(&self, price: i64) -> Result<(util::Uint256, u32), String> {
        self.actor.send_call(HASH, SET_REG_METHOD, vec![stackitem::Item::from(price)])
    }

    // SetRegisterPriceTransaction creates a transaction that sets the new candidate
    // registration price (in GAS). The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The transaction is signed,
    // but not sent to the network, instead it's returned to the caller.
    pub fn set_register_price_transaction(&self, price: i64) -> Result<transaction::Transaction, String> {
        self.actor.make_call(HASH, SET_REG_METHOD, vec![stackitem::Item::from(price)])
    }

    // SetRegisterPriceUnsigned creates a transaction that sets the new candidate
    // registration price (in GAS). The action is successful when transaction
    // ends in HALT state. Notice that this setting can be changed only by the
    // network's committee, so use an appropriate Actor. The transaction is not
    // signed and just returned to the caller.
    pub fn set_register_price_unsigned(&self, price: i64) -> Result<transaction::Transaction, String> {
        self.actor.make_unsigned_call(HASH, SET_REG_METHOD, vec![], vec![stackitem::Item::from(price)])
    }
}

fn reg_script(unreg: bool, k: &keys::PublicKey) -> Vec<u8> {
    let method = if unreg { "unregisterCandidate" } else { "registerCandidate" };
    // We know parameters exactly (unlike with nep17::Transfer), so this can't fail.
    smartcontract::create_call_with_assert_script(HASH, method, vec![stackitem::Item::from(k.to_encoded_point(false).as_bytes())]).unwrap()
}

fn vote_script(account: util::Uint160, vote_to: Option<&keys::PublicKey>) -> Vec<u8> {
    let param = vote_to.map(|k| stackitem::Item::from(k.to_encoded_point(false).as_bytes()));
    // We know parameters exactly (unlike with nep17::Transfer), so this can't fail.
func (c *ContractReader) GetAccountState(account util.Uint160) (*state.NEOBalance, error) {
	itm, err := unwrap.Item(c.invoker.Call(Hash, "getAccountState", account))
	if err != nil {
		return nil, err
	}
	if _, ok := itm.(stackitem.Null); ok {
		return nil, nil
	}
	res := new(state.NEOBalance)
	err = res.FromStackItem(itm)
	if err != nil {
		return nil, err
	}
	return res, nil
}

// GetAllCandidates returns an iterator that allows to retrieve all registered
// validators from it. It depends on the server to provide proper session-based
// iterator, but can also work with expanded one.
func (c *ContractReader) GetAllCandidates() (*ValidatorIterator, error) {
	sess, iter, err := unwrap.SessionIterator(c.invoker.Call(Hash, "getAllCandidates"))
	if err != nil {
		return nil, err
	}

	return &ValidatorIterator{
		client:   c.invoker,
		iterator: iter,
		session:  sess,
	}, nil
}

// GetAllCandidatesExpanded is similar to GetAllCandidates (uses the same NEO
// method), but can be useful if the server used doesn't support sessions and
// doesn't expand iterators. It creates a script that will get num of result
// items from the iterator right in the VM and return them to you. It's only
// limited by VM stack and GAS available for RPC invocations.
func (c *ContractReader) GetAllCandidatesExpanded(num int) ([]result.Validator, error) {
	arr, err := unwrap.Array(c.invoker.CallAndExpandIterator(Hash, "getAllCandidates", num))
	if err != nil {
		return nil, err
	}
	return itemsToValidators(arr)
}

// Next returns the next set of elements from the iterator (up to num of them).
// It can return less than num elements in case iterator doesn't have that many
// or zero elements if the iterator has no more elements or the session is
// expired.
func (v *ValidatorIterator) Next(num int) ([]result.Validator, error) {
	items, err := v.client.TraverseIterator(v.session, &v.iterator, num)
	if err != nil {
		return nil, err
	}
	return itemsToValidators(items)
}

// Terminate closes the iterator session used by ValidatorIterator (if it's
// session-based).
func (v *ValidatorIterator) Terminate() error {
	if v.iterator.ID == nil {
		return nil
	}
	return v.client.TerminateSession(v.session)
}

// GetCandidates returns the list of validators with their vote count. This
// method is mostly useful for historic invocations because the RPC protocol
// provides direct getcandidates call that returns more data and works faster.
// The contract only returns up to 256 candidates in response to this method, so
// if there are more of them on the network you will get a truncated result, use
// GetAllCandidates to solve this problem.
func (c *ContractReader) GetCandidates() ([]result.Validator, error) {
	arr, err := unwrap.Array(c.invoker.Call(Hash, "getCandidates"))
	if err != nil {
		return nil, err
	}
	return itemsToValidators(arr)
}

func itemsToValidators(arr []stackitem.Item) ([]result.Validator, error) {
	res := make([]result.Validator, len(arr))
	for i, itm := range arr {
		str, ok := itm.Value().([]stackitem.Item)
		if !ok {
			return nil, fmt.Errorf("item #%d is not a structure", i)
		}
		if len(str) != 2 {
			return nil, fmt.Errorf("item #%d has wrong length", i)
		}
		b, err := str[0].TryBytes()
		if err != nil {
			return nil, fmt.Errorf("item #%d has wrong key: %w", i, err)
		}
		k, err := keys.NewPublicKeyFromBytes(b, elliptic.P256())
		if err != nil {
			return nil, fmt.Errorf("item #%d has wrong key: %w", i, err)
		}
		votes, err := str[1].TryInteger()
		if err != nil {
			return nil, fmt.Errorf("item #%d has wrong votes: %w", i, err)
		}
		if !votes.IsInt64() {
			return nil, fmt.Errorf("item #%d has too big number of votes", i)
		}
		res[i].PublicKey = *k
		res[i].Votes = votes.Int64()
	}
	return res, nil
}

// GetCommittee returns the list of committee member public keys. This
// method is mostly useful for historic invocations because the RPC protocol
// provides direct getcommittee call that works faster.
func (c *ContractReader) GetCommittee() (keys.PublicKeys, error) {
	return unwrap.ArrayOfPublicKeys(c.invoker.Call(Hash, "getCommittee"))
}

// GetCommitteeAddress returns the committee address.
func (c *ContractReader) GetCommitteeAddress() (util.Uint160, error) {
	return unwrap.Uint160(c.invoker.Call(Hash, "getCommitteeAddress"))
}

// GetNextBlockValidators returns the list of validator keys that will sign the
// next block. This method is mostly useful for historic invocations because the
// RPC protocol provides direct getnextblockvalidators call that provides more
// data and works faster.
func (c *ContractReader) GetNextBlockValidators() (keys.PublicKeys, error) {
	return unwrap.ArrayOfPublicKeys(c.invoker.Call(Hash, "getNextBlockValidators"))
}

// GetGasPerBlock returns the amount of GAS generated in each block.
func (c *ContractReader) GetGasPerBlock() (int64, error) {
	return unwrap.Int64(c.invoker.Call(Hash, "getGasPerBlock"))
}

// GetRegisterPrice returns the price of candidate key registration.
func (c *ContractReader) GetRegisterPrice() (int64, error) {
	return unwrap.Int64(c.invoker.Call(Hash, "getRegisterPrice"))
}

// UnclaimedGas allows to calculate the amount of GAS that will be generated if
// any NEO state change ("claim") is to happen for the given account at the given
// block number. This method is mostly useful for historic invocations because
// the RPC protocol provides direct getunclaimedgas method that works faster.
func (c *ContractReader) UnclaimedGas(account util.Uint160, end uint32) (*big.Int, error) {
	return unwrap.BigInt(c.invoker.Call(Hash, "unclaimedGas", account, end))
}

// RegisterCandidate creates and sends a transaction that adds the given key to
// the list of candidates that can be voted for. The return result from the
// "registerCandidate" method is checked to be true, so transaction fails (with
// FAULT state) if not successful. Notice that for this call to work it must be
// witnessed by the simple account derived from the given key, so use an
// appropriate Actor. The returned values are transaction hash, its
// ValidUntilBlock value and an error if any.
//
// Notice that unlike for all other methods the script for this one is not
// test-executed in its final form because most networks have registration price
// set to be much higher than typical RPC server allows to spend during
// test-execution. This adds some risk that it might fail on-chain, but in
// practice it's not likely to happen if signers are set up correctly.
func (c *Contract) RegisterCandidate(k *keys.PublicKey) (util.Uint256, uint32, error) {
	tx, err := c.RegisterCandidateUnsigned(k)
	if err != nil {
		return util.Uint256{}, 0, err
	}
	return c.actor.SignAndSend(tx)
}

// RegisterCandidateTransaction creates a transaction that adds the given key to
// the list of candidates that can be voted for. The return result from the
// "registerCandidate" method is checked to be true, so transaction fails (with
// FAULT state) if not successful. Notice that for this call to work it must be
// witnessed by the simple account derived from the given key, so use an
// appropriate Actor. The transaction is signed, but not sent to the network,
// instead it's returned to the caller.
//
// Notice that unlike for all other methods the script for this one is not
// test-executed in its final form because most networks have registration price
// set to be much higher than typical RPC server allows to spend during
// test-execution. This adds some risk that it might fail on-chain, but in
// practice it's not likely to happen if signers are set up correctly.
func (c *Contract) RegisterCandidateTransaction(k *keys.PublicKey) (*transaction.Transaction, error) {
	tx, err := c.RegisterCandidateUnsigned(k)
	if err != nil {
		return nil, err
	}
	err = c.actor.Sign(tx)
	if err != nil {
		return nil, err
	}
	return tx, nil
}

// RegisterCandidateUnsigned creates a transaction that adds the given key to
// the list of candidates that can be voted for. The return result from the
// "registerCandidate" method is checked to be true, so transaction fails (with
// FAULT state) if not successful. Notice that for this call to work it must be
// witnessed by the simple account derived from the given key, so use an
// appropriate Actor. The transaction is not signed and just returned to the
// caller.
//
// Notice that unlike for all other methods the script for this one is not
// test-executed in its final form because most networks have registration price
// set to be much higher than typical RPC server allows to spend during
// test-execution. This adds some risk that it might fail on-chain, but in
// practice it's not likely to happen if signers are set up correctly.
func (c *Contract) RegisterCandidateUnsigned(k *keys.PublicKey) (*transaction.Transaction, error) {
	// It's an unregister script intentionally.
	r, err := c.actor.Run(regScript(true, k))
	if err != nil {
		return nil, err
	}
	regPrice, err := c.GetRegisterPrice()
	if err != nil {
		return nil, err
	}
	return c.actor.MakeUnsignedUncheckedRun(regScript(false, k), r.GasConsumed+regPrice, nil)
}

// UnregisterCandidate creates and sends a transaction that removes the key from
// the list of candidates that can be voted for. The return result from the
// "unregisterCandidate" method is checked to be true, so transaction fails (with
// FAULT state) if not successful. Notice that for this call to work it must be
// witnessed by the simple account derived from the given key, so use an
// appropriate Actor. The returned values are transaction hash, its
// ValidUntilBlock value and an error if any.
func (c *Contract) UnregisterCandidate(k *keys.PublicKey) (util.Uint256, uint32, error) {
	return c.actor.SendRun(regScript(true, k))
}

// UnregisterCandidateTransaction creates a transaction that removes the key from
// the list of candidates that can be voted for. The return result from the
// "unregisterCandidate" method is checked to be true, so transaction fails (with
// FAULT state) if not successful. Notice that for this call to work it must be
// witnessed by the simple account derived from the given key, so use an
// appropriate Actor. The transaction is signed, but not sent to the network,
// instead it's returned to the caller.
func (c *Contract) UnregisterCandidateTransaction(k *keys.PublicKey) (*transaction.Transaction, error) {
	return c.actor.MakeRun(regScript(true, k))
}

// UnregisterCandidateUnsigned creates a transaction that removes the key from
// the list of candidates that can be voted for. The return result from the
// "unregisterCandidate" method is checked to be true, so transaction fails (with
// FAULT state) if not successful. Notice that for this call to work it must be
// witnessed by the simple account derived from the given key, so use an
// appropriate Actor. The transaction is not signed and just returned to the
// caller.
func (c *Contract) UnregisterCandidateUnsigned(k *keys.PublicKey) (*transaction.Transaction, error) {
	return c.actor.MakeUnsignedRun(regScript(true, k), nil)
}

func regScript(unreg bool, k *keys.PublicKey) []byte {
	var method = "registerCandidate"

	if unreg {
		method = "unregisterCandidate"
	}

	// We know parameters exactly (unlike with nep17.Transfer), so this can't fail.
	script, _ := smartcontract.CreateCallWithAssertScript(Hash, method, k.Bytes())
	return script
}

// Vote creates and sends a transaction that casts a vote from the given account
// to the given key which can be nil (in which case any previous vote is removed).
// The return result from the "vote" method is checked to be true, so transaction
// fails (with FAULT state) if voting is not successful. The returned values are
// transaction hash, its ValidUntilBlock value and an error if any.
func (c *Contract) Vote(account util.Uint160, voteTo *keys.PublicKey) (util.Uint256, uint32, error) {
	return c.actor.SendRun(voteScript(account, voteTo))
}

// VoteTransaction creates a transaction that casts a vote from the given account
// to the given key which can be nil (in which case any previous vote is removed).
// The return result from the "vote" method is checked to be true, so transaction
// fails (with FAULT state) if voting is not successful. The transaction is signed,
// but not sent to the network, instead it's returned to the caller.
func (c *Contract) VoteTransaction(account util.Uint160, voteTo *keys.PublicKey) (*transaction.Transaction, error) {
	return c.actor.MakeRun(voteScript(account, voteTo))
}

// VoteUnsigned creates a transaction that casts a vote from the given account
// to the given key which can be nil (in which case any previous vote is removed).
// The return result from the "vote" method is checked to be true, so transaction
// fails (with FAULT state) if voting is not successful. The transaction is not
// signed and just returned to the caller.
func (c *Contract) VoteUnsigned(account util.Uint160, voteTo *keys.PublicKey) (*transaction.Transaction, error) {
	return c.actor.MakeUnsignedRun(voteScript(account, voteTo), nil)
}

func voteScript(account util.Uint160, voteTo *keys.PublicKey) []byte {
	var param any

	if voteTo != nil {
		param = voteTo.Bytes()
	}
	// We know parameters exactly (unlike with nep17.Transfer), so this can't fail.
	script, _ := smartcontract.CreateCallWithAssertScript(Hash, "vote", account, param)
	return script
}

// SetGasPerBlock creates and sends a transaction that sets the new amount of
// GAS to be generated in each block. The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The returned values are
// transaction hash, its ValidUntilBlock value and an error if any.
func (c *Contract) SetGasPerBlock(gas int64) (util.Uint256, uint32, error) {
	return c.actor.SendCall(Hash, setGasMethod, gas)
}

// SetGasPerBlockTransaction creates a transaction that sets the new amount of
// GAS to be generated in each block. The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The transaction is signed,
// but not sent to the network, instead it's returned to the caller.
func (c *Contract) SetGasPerBlockTransaction(gas int64) (*transaction.Transaction, error) {
	return c.actor.MakeCall(Hash, setGasMethod, gas)
}

// SetGasPerBlockUnsigned creates a transaction that sets the new amount of
// GAS to be generated in each block. The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The transaction is not
// signed and just returned to the caller.
func (c *Contract) SetGasPerBlockUnsigned(gas int64) (*transaction.Transaction, error) {
	return c.actor.MakeUnsignedCall(Hash, setGasMethod, nil, gas)
}

// SetRegisterPrice creates and sends a transaction that sets the new candidate
// registration price (in GAS). The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The returned values are
// transaction hash, its ValidUntilBlock value and an error if any.
func (c *Contract) SetRegisterPrice(price int64) (util.Uint256, uint32, error) {
	return c.actor.SendCall(Hash, setRegMethod, price)
}

// SetRegisterPriceTransaction creates a transaction that sets the new candidate
// registration price (in GAS). The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The transaction is signed,
// but not sent to the network, instead it's returned to the caller.
func (c *Contract) SetRegisterPriceTransaction(price int64) (*transaction.Transaction, error) {
	return c.actor.MakeCall(Hash, setRegMethod, price)
}

// SetRegisterPriceUnsigned creates a transaction that sets the new candidate
// registration price (in GAS). The action is successful when transaction
// ends in HALT state. Notice that this setting can be changed only by the
// network's committee, so use an appropriate Actor. The transaction is not
// signed and just returned to the caller.
func (c *Contract) SetRegisterPriceUnsigned(price int64) (*transaction.Transaction, error) {
	return c.actor.MakeUnsignedCall(Hash, setRegMethod, nil, price)
}
