use super::IndexerStatus;

fn status(indexed_height: Option<u32>) -> IndexerStatus {
    IndexerStatus {
        indexed_height,
        indexed_hash: None,
        indexed_blocks: 0,
        indexed_transactions: 0,
        indexed_accounts: 0,
        indexed_notifications: 0,
        indexed_notification_accounts: 0,
    }
}

#[test]
fn status_sync_requires_exact_ledger_height() {
    assert!(status(Some(5)).is_synced_with(Some(5)));
    assert!(!status(Some(4)).is_synced_with(Some(5)));
    assert!(!status(Some(6)).is_synced_with(Some(5)));
}

#[test]
fn status_blocks_behind_reports_lag_without_marking_ahead_as_lag() {
    assert_eq!(status(Some(3)).blocks_behind(Some(5)), Some(2));
    assert_eq!(status(Some(7)).blocks_behind(Some(5)), Some(0));
    assert_eq!(status(None).blocks_behind(Some(0)), Some(1));
    assert_eq!(status(Some(0)).blocks_behind(None), None);
}
