use tokio::sync::mpsc;
use tokio::runtime::Runtime;
use anyhow::Result;
use std::sync::Arc;
use std::sync::RwLock;
use crate::neorpc;
use crate::rpcclient::Client;

#[tokio::test]
async fn test_internal_client_close() -> Result<()> {
    let (tx, _rx) = mpsc::channel(1);
    let icl = Client::new_internal(Arc::new(RwLock::new(())), tx).await?;
    icl.close().await;
    assert!(icl.get_error().await.is_none());
    Ok(())
}
