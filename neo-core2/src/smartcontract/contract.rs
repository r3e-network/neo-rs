use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

use neo_crypto::keys::{PublicKey, PublicKeys};
use neo_io::BufBinWriter;
use neo_vm::emit;
use neo_interop::interopnames;

#[derive(Debug)]
struct ContractError(String);

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for ContractError {}

/// Creates an "m out of n" type verification script
/// where n is the length of public_keys. It modifies passed public_keys by
/// sorting them.
pub fn create_multi_sig_redeem_script(m: usize, mut public_keys: PublicKeys) -> Result<Vec<u8>, Box<dyn Error>> {
    if m < 1 {
        return Err(Box::new(ContractError(format!("param m cannot be smaller than 1, got {}", m))));
    }
    if m > public_keys.len() {
        return Err(Box::new(ContractError(format!("length of the signatures ({}) is higher then the number of public keys", m))));
    }
    if m > 1024 {
        return Err(Box::new(ContractError(format!("public key count {} exceeds maximum of length 1024", public_keys.len()))));
    }

    let mut buf = BufBinWriter::new();
    emit::int(&mut buf, m as i64);
    public_keys.sort_by(|a, b| a.cmp(b));
    for pub_key in &public_keys {
        emit::bytes(&mut buf, pub_key.as_bytes());
    }
    emit::int(&mut buf, public_keys.len() as i64);
    emit::syscall(&mut buf, interopnames::SYSTEM_CRYPTO_CHECK_MULTISIG);

    Ok(buf.to_vec())
}

/// Creates an "m out of n" type verification script
/// using public_keys length with the default BFT assumptions of (n - (n-1)/3) for m.
pub fn create_default_multi_sig_redeem_script(public_keys: PublicKeys) -> Result<Vec<u8>, Box<dyn Error>> {
    let n = public_keys.len();
    let m = get_default_honest_node_count(n);
    create_multi_sig_redeem_script(m, public_keys)
}

/// Creates an "m out of n" type verification script
/// using public_keys length with m set to majority.
pub fn create_majority_multi_sig_redeem_script(public_keys: PublicKeys) -> Result<Vec<u8>, Box<dyn Error>> {
    let n = public_keys.len();
    let m = get_majority_honest_node_count(n);
    create_multi_sig_redeem_script(m, public_keys)
}

/// Returns minimum number of honest nodes
/// required for network of size n.
pub fn get_default_honest_node_count(n: usize) -> usize {
    n - (n - 1) / 3
}

/// Returns minimum number of honest nodes
/// required for majority-style agreement.
pub fn get_majority_honest_node_count(n: usize) -> usize {
    n - (n - 1) / 2
}
