use std::error::Error;

use crate::encoding::base58;
use crate::util::Uint160;

const NEO2_PREFIX: u8 = 0x17;
const NEO3_PREFIX: u8 = 0x35;

// Prefix is the byte used to prepend to addresses when encoding them, it can
// be changed and defaults to 53 (0x35), the standard NEO prefix.
static mut PREFIX: u8 = NEO3_PREFIX;

// Uint160ToString returns the "NEO address" from the given Uint160.
pub fn uint160_to_string(u: Uint160) -> String {
    // Don't forget to prepend the Address version 0x17 (23) A
    let mut b = vec![unsafe { PREFIX }];
    b.extend_from_slice(&u.bytes_be());
    base58::check_encode(&b)
}

// StringToUint160 attempts to decode the given NEO address string
// into a Uint160.
pub fn string_to_uint160(s: &str) -> Result<Uint160, Box<dyn Error>> {
    let b = base58::check_decode(s)?;
    if b[0] != unsafe { PREFIX } {
        return Err("wrong address prefix".into());
    }
    Uint160::decode_bytes_be(&b[1..21])
}
