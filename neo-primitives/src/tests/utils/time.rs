use super::*;

#[test]
fn test_time_provider_override() {
    TimeProvider::set_current(TimeSource::fixed_millis(1_600_000_000_000));
    assert_eq!(
        TimeProvider::current().utc_now().timestamp_millis(),
        1_600_000_000_000
    );
    TimeProvider::reset_to_default();
}
