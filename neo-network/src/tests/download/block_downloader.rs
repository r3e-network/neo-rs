use super::*;

#[test]
fn block_request_scheduler_requests_two_protocol_windows() {
    let mut scheduler = BlockRequestScheduler::default();

    let first = scheduler.next_request(0, 5_000).expect("first request");
    let second = scheduler.next_request(0, 5_000).expect("second request");
    let third = scheduler.next_request(0, 5_000);

    assert_eq!(first, BlockRequest::new(1, 500));
    assert_eq!(second, BlockRequest::new(501, 500));
    assert!(third.is_none());
}

#[test]
fn block_request_scheduler_resumes_from_persisted_tip() {
    let mut scheduler = BlockRequestScheduler::default();
    scheduler
        .next_request(42, 100)
        .expect("request after durable tip");

    assert_eq!(scheduler.requested_to(), 100);
}

#[test]
fn block_request_scheduler_resets_when_caught_up() {
    let mut scheduler = BlockRequestScheduler::default();
    scheduler.next_request(0, 100).expect("request");

    assert!(scheduler.next_request(100, 100).is_none());
    assert_eq!(scheduler.requested_to(), 100);
    assert_eq!(scheduler.stall_ticks(), 0);
}

#[test]
fn block_request_scheduler_rewinds_after_stall_limit() {
    let mut scheduler = BlockRequestScheduler::default();
    scheduler.next_request(0, 5_000).expect("first");
    scheduler.next_request(0, 5_000).expect("second");

    for _ in 0..BlockRequestScheduler::STALL_LIMIT {
        scheduler.record_tick(0, 5_000);
    }

    let retry = scheduler.next_request(0, 5_000).expect("retry after stall");
    assert_eq!(retry, BlockRequest::new(1, 500));
}
