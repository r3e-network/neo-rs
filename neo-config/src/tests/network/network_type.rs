use super::*;

#[test]
fn test_network_magic() {
    assert_eq!(NetworkType::MainNet.magic(), 860833102);
    assert_eq!(NetworkType::TestNet.magic(), 894710606);
}

#[test]
fn test_network_from_str() {
    assert_eq!(
        "mainnet".parse::<NetworkType>().ok(),
        Some(NetworkType::MainNet)
    );
    assert_eq!(
        "TESTNET".parse::<NetworkType>().ok(),
        Some(NetworkType::TestNet)
    );
    assert_eq!(
        "private".parse::<NetworkType>().ok(),
        Some(NetworkType::Private)
    );
    assert!("unknown".parse::<NetworkType>().is_err());
}
