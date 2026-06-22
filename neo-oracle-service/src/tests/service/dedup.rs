use super::super::{DEDUP_CACHE_TTL, FINISHED_CACHE_TTL, OracleService, OracleServiceSettings};
use neo_config::ProtocolSettings;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

fn oracle_service(enable_deduplication: bool) -> OracleService {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let settings = Arc::new(ProtocolSettings::testnet());
    let system = neo_system::Node::new(Arc::clone(&settings), None, None).expect("neo system");
    let oracle_settings = OracleServiceSettings {
        network: settings.network,
        enable_deduplication,
        ..Default::default()
    };

    OracleService::new(oracle_settings, Arc::new(system)).expect("oracle service")
}

#[test]
fn dedup_cache_tracks_in_flight_and_recent_urls() {
    let service = oracle_service(true);
    let url = "https://oracle.example/request";

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.in_flight_count(), 1);
    assert_eq!(service.dedup_cache_size(), 0);

    assert!(service.is_duplicate_request(2, url));

    service.mark_request_completed(1, url);
    assert_eq!(service.in_flight_count(), 0);
    assert_eq!(service.dedup_cache_size(), 1);
    assert!(service.is_duplicate_request(3, url));
}

#[test]
fn dedup_cache_prunes_expired_urls_before_checking_recent_duplicates() {
    let service = oracle_service(true);
    let url = "https://oracle.example/expired";
    let expired = SystemTime::now() - DEDUP_CACHE_TTL - Duration::from_secs(1);
    service
        .dedup
        .lock()
        .completed
        .insert_at(url.to_string(), expired);

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.dedup_cache_size(), 0);
    assert_eq!(service.in_flight_count(), 1);
}

#[test]
fn dedup_cache_retains_future_urls_without_marking_them_recent() {
    let service = oracle_service(true);
    let url = "https://oracle.example/future";
    let future = SystemTime::now() + Duration::from_secs(60);
    service
        .dedup
        .lock()
        .completed
        .insert_at(url.to_string(), future);

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.dedup_cache_size(), 1);
    assert_eq!(service.in_flight_count(), 1);
}

#[test]
fn disabled_deduplication_does_not_track_urls() {
    let service = oracle_service(false);
    let url = "https://oracle.example/request";

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.in_flight_count(), 0);
    assert_eq!(service.dedup_cache_size(), 0);
}

#[test]
fn finished_cache_entries_clear_on_timer_sweep() {
    let service = oracle_service(true);
    let request_id = 42;

    service
        .finished_cache
        .lock()
        .insert_at(request_id, SystemTime::UNIX_EPOCH);
    assert!(service.is_request_finished(request_id));

    service.cleanup_finished_cache(SystemTime::UNIX_EPOCH + FINISHED_CACHE_TTL);
    assert!(service.is_request_finished(request_id));

    service.cleanup_finished_cache(
        SystemTime::UNIX_EPOCH + FINISHED_CACHE_TTL + Duration::from_secs(1),
    );
    assert!(!service.is_request_finished(request_id));
}
