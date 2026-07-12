use super::*;
use tokio::sync::mpsc;

#[tokio::test]
async fn try_send_raw_drops_on_full_channel_without_blocking() {
    let (tx, _rx) = mpsc::channel::<RemoteNodeCommand>(2);
    let addr = "10.0.0.2:1002".parse().expect("addr");
    let handle = RemoteNodeHandle::from_parts(tx, PeerId::new(), addr);
    assert!(handle.try_send_raw(vec![1]).is_ok());
    assert!(handle.try_send_raw(vec![2]).is_ok());
    // The channel is full and `_rx` is never polled: try_send must return
    // Err immediately rather than parking the shared broadcast loop.
    let res = tokio::time::timeout(std::time::Duration::from_millis(200), async {
        handle.try_send_raw(vec![3])
    })
    .await
    .expect("try_send must not block on a full channel");
    assert!(res.is_err(), "a full peer channel must drop, not block");
}

#[tokio::test]
async fn fetch_headers_by_index_rejects_counts_above_protocol_cap() {
    let (tx, _rx) = mpsc::channel::<RemoteNodeCommand>(1);
    let addr = "10.0.0.2:1002".parse().expect("addr");
    let handle = RemoteNodeHandle::from_parts(tx, PeerId::new(), addr);

    let error = handle
        .fetch_headers_by_index(HeaderRequest::new(0, HeaderRequest::MAX_COUNT + 1))
        .await
        .expect_err("counts above the GetHeaders cap must be rejected");

    match error {
        crate::error::NetworkError::Protocol(detail) => {
            assert!(detail.contains("GetHeaders"), "{detail}");
            assert!(detail.contains("2000"), "{detail}");
        }
        other => panic!("expected protocol error, got {other}"),
    }
}
