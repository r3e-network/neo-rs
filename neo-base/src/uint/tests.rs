use super::{address::AddressVersion, UInt160, UInt256};
use crate::hash::hash160;
use alloc::format;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json;

#[test]
fn display_matches_csharp_format() {
    let value = UInt160::from_slice(&[0x52; 20]).unwrap();
    assert_eq!(
        format!("{value}"),
        "0x5252525252525252525252525252525252525252"
    );
}

#[test]
fn address_roundtrip_from_csharp_vectors() {
    let script = STANDARD
        .decode("DCECozKyXb9hGPwlv2Tw2DALu2I7eDRDcazwy1ByffMtnbNBVuezJw==")
        .expect("base64");
    let script_hash = UInt160::from_slice(&hash160(&script)).unwrap();
    let version = AddressVersion::MAINNET;
    let address = script_hash.to_address(version);
    assert_eq!(address, "NRPf2BLaP595UFybH1nwrExJSt5ZGbKnjd");
    let parsed = UInt160::from_address(&address, version).unwrap();
    assert_eq!(parsed, script_hash);
    assert_eq!(
        format!("{script_hash}"),
        "0x8618383E5B58C50C66BC8A8E8E43725DC41C153C"
    );
}

#[test]
fn serde_roundtrip_uint256() {
    let value = UInt256::from_slice(&[0xAB; 32]).unwrap();
    let serialized = serde_json::to_string(&value).expect("serialize");
    let expected = format!("\"{value}\"");
    assert_eq!(serialized, expected);
    let deserialized: UInt256 = serde_json::from_str(&serialized).expect("deserialize");
    assert_eq!(deserialized, value);
}
