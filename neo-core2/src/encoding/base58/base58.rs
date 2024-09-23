use std::error::Error;
use std::fmt;

use bytes::Bytes;
use base58::FromBase58;
use base58::ToBase58;
use crate::crypto::hash;

// Custom error type for base58 decoding errors
#[derive(Debug)]
struct Base58Error {
    details: String,
}

impl Base58Error {
    fn new(msg: &str) -> Base58Error {
        Base58Error{details: msg.to_string()}
    }
}

impl fmt::Display for Base58Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for Base58Error {
    fn description(&self) -> &str {
        &self.details
    }
}

// CheckDecode implements base58-encoded string decoding with a hash-based
// checksum check.
pub fn check_decode(s: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut b = s.from_base58()?;
    
    if b.len() < 5 {
        return Err(Box::new(Base58Error::new("invalid base-58 check string: missing checksum")));
    }

    let checksum = hash::checksum(&b[..b.len() - 4]);
    if &checksum[..] != &b[b.len() - 4..] {
        return Err(Box::new(Base58Error::new("invalid base-58 check string: invalid checksum")));
    }

    // Strip the 4 byte long hash.
    b.truncate(b.len() - 4);

    Ok(b)
}

// CheckEncode encodes the given byte slice into a base58 string with a hash-based
// checksum appended to it.
pub fn check_encode(b: &[u8]) -> String {
    let mut b = b.to_vec();
    let checksum = hash::checksum(&b);
    b.extend_from_slice(&checksum);

    b.to_base58()
}
