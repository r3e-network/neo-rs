/*
Package rolemgmt allows to work with the native RoleManagement contract via RPC.

Safe methods are encapsulated into ContractReader structure while Contract provides
various methods to perform the only RoleManagement state-changing call.
*/

use crate::core::native::nativehashes;
use crate::core::native::noderoles;
use crate::core::transaction;
use crate::crypto::keys;
use crate::neorpc::result;
use crate::rpcclient::unwrap;
use crate::util;
use std::error::Error;

// Invoker is used by ContractReader to call various methods.
pub trait Invoker {
    fn call(&self, contract: util::Uint160, operation: &str, params: &[impl std::any::Any]) -> Result<result::Invoke, Box<dyn Error>>;
}

// Actor is used by Contract to create and send transactions.
pub trait Actor: Invoker {
    fn make_call(&self, contract: util::Uint160, method: &str, params: &[impl std::any::Any]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn make_unsigned_call(&self, contract: util::Uint160, method: &str, attrs: &[transaction::Attribute], params: &[impl std::any::Any]) -> Result<transaction::Transaction, Box<dyn Error>>;
    fn send_call(&self, contract: util::Uint160, method: &str, params: &[impl std::any::Any]) -> Result<(util::Uint256, u32), Box<dyn Error>>;
}

// Hash stores the hash of the native RoleManagement contract.
pub const HASH: util::Uint160 = nativehashes::ROLE_MANAGEMENT;

const DESIGNATE_METHOD: &str = "designateAsRole";

// ContractReader provides an interface to call read-only RoleManagement
// contract's methods.
pub struct ContractReader {
    invoker: Box<dyn Invoker>,
}

// Contract represents a RoleManagement contract client that can be used to
// invoke all of its methods.
pub struct Contract {
    reader: ContractReader,
    actor: Box<dyn Actor>,
}

// DesignationEvent represents an event emitted by RoleManagement contract when
// a new role designation is done.
pub struct DesignationEvent {
    role: noderoles::Role,
    block_index: u32,
}

// NewReader creates an instance of ContractReader that can be used to read
// data from the contract.
pub fn new_reader(invoker: Box<dyn Invoker>) -> ContractReader {
    ContractReader { invoker }
}

// New creates an instance of Contract to perform actions using
// the given Actor. Notice that RoleManagement's state can be changed
// only by the network's committee, so the Actor provided must be a committee
// actor for designation methods to work properly.
pub fn new(actor: Box<dyn Actor>) -> Contract {
    Contract {
        reader: new_reader(actor.clone()),
        actor,
    }
}

// GetDesignatedByRole returns the list of the keys designated to serve for the
// given role at the given height. The list can be empty if no keys are
// configured for this role/height.
impl ContractReader {
    pub fn get_designated_by_role(&self, role: noderoles::Role, index: u32) -> Result<keys::PublicKeys, Box<dyn Error>> {
        unwrap::array_of_public_keys(self.invoker.call(HASH, "getDesignatedByRole", &[role as i64, index]))
    }
}

// DesignateAsRole creates and sends a transaction that sets the keys used for
// the given node role. The action is successful when transaction ends in HALT
// state. The returned values are transaction hash, its ValidUntilBlock value
// and an error if any.
impl Contract {
    pub fn designate_as_role(&self, role: noderoles::Role, pubs: keys::PublicKeys) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.actor.send_call(HASH, DESIGNATE_METHOD, &[role as i32, pubs])
    }

    // DesignateAsRoleTransaction creates a transaction that sets the keys for the
    // given node role. This transaction is signed, but not sent to the network,
    // instead it's returned to the caller.
    pub fn designate_as_role_transaction(&self, role: noderoles::Role, pubs: keys::PublicKeys) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_call(HASH, DESIGNATE_METHOD, &[role as i32, pubs])
    }

    // DesignateAsRoleUnsigned creates a transaction that sets the keys for the
    // given node role. This transaction is not signed and just returned to the
    // caller.
    pub fn designate_as_role_unsigned(&self, role: noderoles::Role, pubs: keys::PublicKeys) -> Result<transaction::Transaction, Box<dyn Error>> {
        self.actor.make_unsigned_call(HASH, DESIGNATE_METHOD, &[], &[role as i32, pubs])
    }
}
