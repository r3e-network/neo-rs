extern crate hex;
extern crate base58;
extern crate assert;
extern crate require;

use hex::decode;
use base58::{CheckEncode, CheckDecode};
use assert::{assert_eq, assert_ne};
use require::{require_eq, require_no_err};

#[test]
fn test_check_encode_decode() {
    let b58_csum_encoded = "KxhEDBQyyEFymvfJD96q8stMbJMbZUb6D1PmXqBWZDU2WvbvVs9o";
    let b58_csum_decoded_hex = "802bfe58ab6d9fd575bdc3a624e4825dd2b375d64ac033fbc46ea79dbab4f69a3e01";

    let b58_csum_decoded = decode(b58_csum_decoded_hex).unwrap();
    let encoded = CheckEncode(&b58_csum_decoded);
    let decoded = CheckDecode(b58_csum_encoded).unwrap();
    assert_eq!(encoded, b58_csum_encoded);
    assert_eq!(decoded, b58_csum_decoded);
}

#[test]
fn test_check_decode_failures() {
    let bad_base58 = "BASE%*";
    assert!(CheckDecode(bad_base58).is_err());
    
    let short_base58 = "THqY";
    assert!(CheckDecode(short_base58).is_err());
    
    let bad_csum = "KxhEDBQyyEFymvfJD96q8stMbJMbZUb6D1PmXqBWZDU2WvbvVs9A";
    assert!(CheckDecode(bad_csum).is_err());
}

#[test]
fn test_base58_leading_zeroes() {
    let buf = vec![0, 0, 0, 1];
    let b58 = CheckEncode(&buf);
    let dec = CheckDecode(&b58).unwrap();
    require_no_err!(dec);
    require_eq!(buf, dec);
}
