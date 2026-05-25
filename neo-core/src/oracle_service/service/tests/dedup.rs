use super::super::{OracleService, OracleServiceSettings, DEDUP_CACHE_TTL};
use crate::protocol_settings::ProtocolSettings;
use std::time::{Duration, SystemTime};

fn oracle_service(enable_deduplication: bool) -> OracleService {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let settings = ProtocolSettings::testnet();
    let system =
        crate::neo_system::NeoSystem::new(settings.clone(), None, None).expect("neo system");
    let oracle_settings = OracleServiceSettings {
        network: settings.network,
        enable_deduplication,
        ..Default::default()
    };

    OracleService::new(oracle_settings, system).expect("oracle service")
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
    service.dedup_cache.lock().insert(url.to_string(), expired);

    assert!(!service.is_duplicate_request(1, url));
    assert_eq!(service.dedup_cache_size(), 0);
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
