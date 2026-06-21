use super::*;

#[test]
fn classify_already_seen_for_past_height() {
    assert_eq!(
        ImportDisposition::classify_import_block(10, 5),
        ImportDisposition::AlreadySeen
    );
    assert_eq!(
        ImportDisposition::classify_import_block(10, 10),
        ImportDisposition::AlreadySeen
    );
}

#[test]
fn classify_next_expected_when_in_sequence() {
    assert_eq!(
        ImportDisposition::classify_import_block(7, 8),
        ImportDisposition::NextExpected
    );
}

#[test]
fn classify_future_gap_for_skip() {
    assert_eq!(
        ImportDisposition::classify_import_block(3, 8),
        ImportDisposition::FutureGap
    );
}

#[test]
fn schedule_idle_only_when_more_pending_without_backlog() {
    assert!(should_schedule_reverify_idle(true, false));
    assert!(!should_schedule_reverify_idle(false, false));
    assert!(!should_schedule_reverify_idle(true, true));
}
