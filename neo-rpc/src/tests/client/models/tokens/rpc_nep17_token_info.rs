use super::*;

#[test]
fn token_info_roundtrip() {
    let info = RpcNep17TokenInfo {
        name: "TestToken".to_string(),
        symbol: "TT".to_string(),
        decimals: 8,
        total_supply: BigInt::from(1_000_000),
        balance: Some(BigInt::from(42)),
        last_updated_block: Some(123),
    };
    let json = serde_json::to_string(&info).unwrap();
    let parsed: RpcNep17TokenInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, info.name);
    assert_eq!(parsed.symbol, info.symbol);
    assert_eq!(parsed.decimals, info.decimals);
    assert_eq!(parsed.total_supply, info.total_supply);
    assert_eq!(parsed.balance, info.balance);
    assert_eq!(parsed.last_updated_block, info.last_updated_block);
}
