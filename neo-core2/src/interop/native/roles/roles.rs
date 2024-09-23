/// Package roles provides an interface to RoleManagement native contract.
/// Role management contract is used by committee to designate some nodes as
/// providing some service on the network.

use crate::interop;
use crate::interop::contract;
use crate::interop::neogointernal;

// Hash represents RoleManagement contract hash.
const HASH: &str = "\u{e2}\u{95}\u{e3}\u{91}\u{54}\u{4c}\u{17}\u{8a}\u{d9}\u{4f}\u{03}\u{ec}\u{4d}\u{cd}\u{ff}\u{78}\u{53}\u{4e}\u{cf}\u{49}";

// Role represents a node role.
#[derive(Debug, Clone, Copy)]
enum Role {
    StateValidator = 4,
    Oracle = 8,
    NeoFSAlphabet = 16,
    P2PNotary = 32,
}

// GetDesignatedByRole represents `getDesignatedByRole` method of RoleManagement native contract.
fn get_designated_by_role(r: Role, height: u32) -> Vec<interop::PublicKey> {
    neogointernal::call_with_token(
        HASH,
        "getDesignatedByRole",
        contract::ReadStates as i32,
        r as u8,
        height,
    )
    .try_into()
    .expect("Failed to convert to Vec<interop::PublicKey>")
}

// DesignateAsRole represents `designateAsRole` method of RoleManagement native contract.
fn designate_as_role(r: Role, pubs: Vec<interop::PublicKey>) {
    neogointernal::call_with_token_no_ret(
        HASH,
        "designateAsRole",
        (contract::States | contract::AllowNotify) as i32,
        r as u8,
        pubs,
    );
}
