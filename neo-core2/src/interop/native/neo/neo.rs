/*
Package neo provides an interface to NeoToken native contract.
NEO token is special, it's not just a regular NEP-17 contract, it also
provides access to chain-specific settings and implements committee
voting system.
*/

use crate::interop::contract;
use crate::interop::iterator;
use crate::interop::neogointernal;
use crate::interop::{self, Hash160, PublicKey};

// AccountState contains info about a NEO holder.
#[derive(Debug)]
struct AccountState {
    balance: i32,
    height: i32,
    vote_to: PublicKey,
    last_gas_per_vote: i32,
}

// Hash represents NEO contract hash.
const HASH: &str = "\xf5\x63\xea\x40\xbc\x28\x3d\x4d\x0e\x05\xc4\x8e\xa3\x05\xb3\xf2\xa0\x73\x40\xef";

// Symbol represents `symbol` method of NEO native contract.
fn symbol() -> String {
    neogointernal::call_with_token(HASH, "symbol", contract::NoneFlag as i32).unwrap()
}

// Decimals represents `decimals` method of NEO native contract.
fn decimals() -> i32 {
    neogointernal::call_with_token(HASH, "decimals", contract::NoneFlag as i32).unwrap()
}

// TotalSupply represents `totalSupply` method of NEO native contract.
fn total_supply() -> i32 {
    neogointernal::call_with_token(HASH, "totalSupply", contract::ReadStates as i32).unwrap()
}

// BalanceOf represents `balanceOf` method of NEO native contract.
fn balance_of(addr: Hash160) -> i32 {
    neogointernal::call_with_token(HASH, "balanceOf", contract::ReadStates as i32, addr).unwrap()
}

// Transfer represents `transfer` method of NEO native contract.
fn transfer(from: Hash160, to: Hash160, amount: i32, data: impl std::any::Any) -> bool {
    neogointernal::call_with_token(HASH, "transfer", contract::All as i32, from, to, amount, data).unwrap()
}

// GetCommittee represents `getCommittee` method of NEO native contract.
fn get_committee() -> Vec<PublicKey> {
    neogointernal::call_with_token(HASH, "getCommittee", contract::ReadStates as i32).unwrap()
}

// GetCandidates represents `getCandidates` method of NEO native contract. It
// returns up to 256 candidates. Use GetAllCandidates in case if you need the
// whole set of candidates.
fn get_candidates() -> Vec<Candidate> {
    neogointernal::call_with_token(HASH, "getCandidates", contract::ReadStates as i32).unwrap()
}

// GetAllCandidates represents `getAllCandidates` method of NEO native contract.
// It returns Iterator over the whole set of Neo candidates sorted by public key
// bytes. Each iterator value can be cast to Candidate. Use iterator interop
// package to work with the returned Iterator.
fn get_all_candidates() -> iterator::Iterator {
    neogointernal::call_with_token(HASH, "getAllCandidates", contract::ReadStates as i32).unwrap()
}

// GetCandidateVote represents `getCandidateVote` method of NEO native contract.
// It returns -1 if the candidate hasn't been registered or voted for and the
// overall candidate votes otherwise.
fn get_candidate_vote(pub: PublicKey) -> i32 {
    neogointernal::call_with_token(HASH, "getCandidateVote", contract::ReadStates as i32, pub).unwrap()
}

// GetNextBlockValidators represents `getNextBlockValidators` method of NEO native contract.
fn get_next_block_validators() -> Vec<PublicKey> {
    neogointernal::call_with_token(HASH, "getNextBlockValidators", contract::ReadStates as i32).unwrap()
}

// GetGASPerBlock represents `getGasPerBlock` method of NEO native contract.
fn get_gas_per_block() -> i32 {
    neogointernal::call_with_token(HASH, "getGasPerBlock", contract::ReadStates as i32).unwrap()
}

// SetGASPerBlock represents `setGasPerBlock` method of NEO native contract.
fn set_gas_per_block(amount: i32) {
    neogointernal::call_with_token_no_ret(HASH, "setGasPerBlock", contract::States as i32, amount);
}

// GetRegisterPrice represents `getRegisterPrice` method of NEO native contract.
fn get_register_price() -> i32 {
    neogointernal::call_with_token(HASH, "getRegisterPrice", contract::ReadStates as i32).unwrap()
}

// SetRegisterPrice represents `setRegisterPrice` method of NEO native contract.
fn set_register_price(amount: i32) {
    neogointernal::call_with_token_no_ret(HASH, "setRegisterPrice", contract::States as i32, amount);
}

// RegisterCandidate represents `registerCandidate` method of NEO native contract.
fn register_candidate(pub: PublicKey) -> bool {
    neogointernal::call_with_token(HASH, "registerCandidate", contract::States as i32, pub).unwrap()
}

// UnregisterCandidate represents `unregisterCandidate` method of NEO native contract.
fn unregister_candidate(pub: PublicKey) -> bool {
    neogointernal::call_with_token(HASH, "unregisterCandidate", contract::States as i32, pub).unwrap()
}

// Vote represents `vote` method of NEO native contract.
fn vote(addr: Hash160, pub: PublicKey) -> bool {
    neogointernal::call_with_token(HASH, "vote", contract::States as i32, addr, pub).unwrap()
}

// UnclaimedGAS represents `unclaimedGas` method of NEO native contract.
fn unclaimed_gas(addr: Hash160, end: i32) -> i32 {
    neogointernal::call_with_token(HASH, "unclaimedGas", contract::ReadStates as i32, addr, end).unwrap()
}

// GetAccountState represents `getAccountState` method of NEO native contract.
fn get_account_state(addr: Hash160) -> AccountState {
    neogointernal::call_with_token(HASH, "getAccountState", contract::ReadStates as i32, addr).unwrap()
}

// GetCommitteeAddress represents `getCommitteeAddress` method of NEO native contract.
fn get_committee_address() -> Hash160 {
    neogointernal::call_with_token(HASH, "getCommitteeAddress", contract::ReadStates as i32).unwrap()
}
