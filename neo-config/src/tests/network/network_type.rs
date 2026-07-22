use super::*;

#[test]
fn test_network_magic() {
    assert_eq!(NetworkType::MainNet.canonical_magic(), Some(860833102));
    assert_eq!(NetworkType::TestNet.canonical_magic(), Some(894710606));
    assert_eq!(NetworkType::Private.canonical_magic(), None);
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
