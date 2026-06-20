use super::super::super::config::{ObservabilityErrorEndpoint, ObservabilitySection};
use super::super::ObservabilityRuntime;

#[tokio::test]
async fn runtime_can_be_constructed_and_dropped_inside_tokio_context() {
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: false,
        error_endpoints: vec![ObservabilityErrorEndpoint {
            kind: Some("custom_json".to_string()),
            url: Some("https://errors.example.com/neo-node".to_string()),
            ..ObservabilityErrorEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };

    let runtime = ObservabilityRuntime::from_config(&config, 0x3554_334E)
        .expect("runtime config")
        .expect("observability enabled");

    drop(runtime);
}
