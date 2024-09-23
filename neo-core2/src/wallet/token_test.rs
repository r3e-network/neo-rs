use std::str::FromStr;
use serde_json;

use neo_core2::smartcontract::manifest;
use neo_core2::types::Uint160;
use neo_core2::wallet::Token;

#[test]
fn test_token_serialize_deserialize() {
    // From the https://neo-python.readthedocs.io/en/latest/prompt.html#import-nep5-compliant-token
    let h = Uint160::from_str("f8d448b227991cf07cb96a6f9c0322437f1599b9").unwrap();

    let tok = Token::new(h, "NEP-17 standard token", "NEPT", 8, manifest::NEP17_STANDARD_NAME);
    assert_eq!("NEP-17 standard token", tok.name);
    assert_eq!("NEPT", tok.symbol);
    assert_eq!(8, tok.decimals);
    assert_eq!(h, tok.hash);
    assert_eq!("NcqKahsZ93ZyYS5bep8G2TY1zRB7tfUPdK", tok.address());

    let data = serde_json::to_string(&tok).unwrap();

    let actual: Token = serde_json::from_str(&data).unwrap();
    assert_eq!(tok, actual);
}
