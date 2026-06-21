use super::*;

#[tokio::test]
async fn empty_provider_is_unloaded() {
    let p = WalletProvider::new();
    assert!(!p.is_loaded().await);
}

#[tokio::test]
async fn clear_is_idempotent() {
    let p = WalletProvider::new();
    p.clear().await;
    assert!(!p.is_loaded().await);
}
