use std::error::Error;
use std::fmt;

use elliptic_curve::sec1::ToEncodedPoint;
use k256::ecdsa::{signature::Verifier, VerifyingKey};
use k256::elliptic_curve::sec1::FromEncodedPoint;
use k256::elliptic_curve::FieldBytes;
use k256::elliptic_curve::sec1::EncodedPoint;
use sha2::{Digest, Sha256};

use crate::core::fee;
use crate::core::interop::Context;
use crate::crypto::hash;
use crate::vm::{self, stackitem::StackItem};

#[derive(Debug)]
struct GasLimitExceeded;

impl fmt::Display for GasLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "gas limit exceeded")
    }
}

impl Error for GasLimitExceeded {}

// ECDSASecp256r1CheckMultisig checks multiple ECDSA signatures at once using
// Secp256r1 elliptic curve.
pub fn ecdsa_secp256r1_check_multisig(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let pkeys = ic.vm.estack().pop_sig_elements().map_err(|e| format!("wrong key parameters: {}", e))?;
    if !ic.vm.add_gas(ic.base_exec_fee() * fee::ECDSA_VERIFY_PRICE * pkeys.len() as i64) {
        return Err(Box::new(GasLimitExceeded));
    }
    let sigs = ic.vm.estack().pop_sig_elements().map_err(|e| format!("wrong signature parameters: {}", e))?;
    if pkeys.len() < sigs.len() {
        return Err("more signatures than there are keys".into());
    }
    let hash = hash::net_sha256(ic.network, &ic.container).to_bytes_be();
    let sigok = vm::check_multisig_par(&k256::Secp256r1, &hash, &pkeys, &sigs);
    ic.vm.estack().push_item(StackItem::Bool(sigok));
    Ok(())
}

// ECDSASecp256r1CheckSig checks ECDSA signature using Secp256r1 elliptic curve.
pub fn ecdsa_secp256r1_check_sig(ic: &mut Context) -> Result<(), Box<dyn Error>> {
    let keyb = ic.vm.estack().pop().bytes();
    let signature = ic.vm.estack().pop().bytes();
    let pkey = VerifyingKey::from_encoded_point(&EncodedPoint::from_bytes(&keyb).map_err(|_| "invalid public key")?)
        .map_err(|_| "invalid public key")?;
    let res = pkey.verify_hashable(&signature, ic.network, &ic.container).is_ok();
    ic.vm.estack().push_item(StackItem::Bool(res));
    Ok(())
}
