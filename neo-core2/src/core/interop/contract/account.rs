use std::error::Error;
use std::fmt;
use std::convert::TryInto;
use elliptic_curve::sec1::ToEncodedPoint;
use k256::elliptic_curve::sec1::FromEncodedPoint;
use k256::PublicKey;
use crate::config;
use crate::core::fee;
use crate::core::interop::Context;
use crate::crypto::hash;
use crate::crypto::keys;
use crate::smartcontract;
use crate::vm::stackitem::{StackItem, ByteArray};

#[derive(Debug)]
struct ContractError {
    details: String,
}

impl ContractError {
    fn new(msg: &str) -> ContractError {
        ContractError{details: msg.to_string()}
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for ContractError {
    fn description(&self) -> &str {
        &self.details
    }
}

// CreateMultisigAccount calculates multisig contract scripthash for a
// given m and a set of public keys.
pub fn create_multisig_account(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let m = ic.vm.estack().pop().bigint();
    let mu64 = m.to_u64().ok_or_else(|| ContractError::new("m must be positive and fit int32"))?;
    if mu64 > i32::MAX as u64 {
        return Err(Box::new(ContractError::new("m must be positive and fit int32")));
    }
    let arr = ic.vm.estack().pop().array();
    let mut pubs = Vec::with_capacity(arr.len());
    for pk in arr {
        let p = PublicKey::from_sec1_bytes(pk.value().as_slice())?;
        pubs.push(p);
    }
    let invoke_fee = if ic.is_hardfork_enabled(config::HFAspidochelone) {
        fee::ECDSA_VERIFY_PRICE * pubs.len() as i64
    } else {
        1 << 8
    } * ic.base_exec_fee();
    if !ic.vm.add_gas(invoke_fee) {
        return Err(Box::new(ContractError::new("gas limit exceeded")));
    }
    let script = smartcontract::create_multisig_redeem_script(mu64 as usize, &pubs)?;
    ic.vm.estack().push_item(ByteArray::new(hash::hash160(&script).as_bytes().to_vec()));
    Ok(())
}

// CreateStandardAccount calculates contract scripthash for a given public key.
pub fn create_standard_account(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let h = ic.vm.estack().pop().bytes();
    let p = PublicKey::from_sec1_bytes(&h)?;
    let invoke_fee = if ic.is_hardfork_enabled(config::HFAspidochelone) {
        fee::ECDSA_VERIFY_PRICE
    } else {
        1 << 8
    } * ic.base_exec_fee();
    if !ic.vm.add_gas(invoke_fee) {
        return Err(Box::new(ContractError::new("gas limit exceeded")));
    }
    ic.vm.estack().push_item(ByteArray::new(p.to_encoded_point(false).as_bytes().to_vec()));
    Ok(())
}
