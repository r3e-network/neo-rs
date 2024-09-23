use crate::interop;
use crate::interop::native::std;
use crate::interop::runtime;

// ToHash160 is a utility function that converts a Neo address to its hash
// (160 bit BE value in a 20 byte slice). When parameter is known at compile time
// (it's a constant string) the output is calculated by the compiler and this
// function is optimized out completely. Otherwise, standard library and system
// calls are used to perform the conversion and checks (panic will happen on
// invalid input).
pub fn to_hash160(address: &str) -> interop::Hash160 {
    let b = std::base58_check_decode(address.as_bytes());
    if b.len() != interop::HASH160_LEN + 1 {
        panic!("invalid address length");
    }
    if b[0] as i32 != runtime::get_address_version() {
        panic!("invalid address prefix");
    }
    b[1..21].to_vec()
}

// FromHash160 is a utility function that converts given Hash160 to
// Base58-encoded Neo address.
pub fn from_hash160(hash: interop::Hash160) -> String {
    if hash.len() != interop::HASH160_LEN {
        panic!("invalid Hash160 length");
    }
    let mut res = vec![runtime::get_address_version() as u8];
    res.extend_from_slice(&hash);
    std::base58_check_encode(&res)
}
