use neo_core::smart_contract::native::{GasToken, NativeContract, NeoToken};

#[test]
fn neo_token_hash_matches_reference() {
    let neo = NeoToken::new();
    assert_eq!(
        hex::encode(neo.hash().to_array()),
        "ef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"
    );
    assert_eq!(neo.symbol(), "NEO");
    assert_eq!(neo.decimals(), 0);
}

#[test]
fn gas_token_hash_matches_reference() {
    let gas = GasToken::new();
    assert_eq!(
        hex::encode(gas.hash().to_array()),
        "d2a4cff31913016155e38e474a2c06d08be276cf"
    );
    assert_eq!(gas.symbol(), "GAS");
    assert_eq!(gas.decimals(), 8);
}
