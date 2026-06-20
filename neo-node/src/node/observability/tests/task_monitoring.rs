use super::super::super::config::{ObservabilityErrorEndpoint, ObservabilitySection};
use super::super::ObservabilityRuntime;

use std::time::Duration;

use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[test]
fn node_long_running_background_tasks_are_spawned_under_observability_monitoring() {
    let node_source = include_str!("../../../node.rs");
    assert!(
        node_source.contains("spawn_daemon_task"),
        "node composition should centralize long-running task spawning so observability can monitor exits and panics"
    );

    for task_name in [
        "blockchain_service",
        "p2p_service",
        "inventory_relay",
        "consensus_driver",
        "telemetry_metrics",
        "network_height_advertiser",
        "indexer_runtime",
    ] {
        assert!(
            node_source.contains(&format!("\"{task_name}\"")),
            "node task {task_name} should be spawned with an observability task name"
        );
    }
}

#[test]
fn heartbeat_tasks_are_spawned_under_observability_monitoring() {
    let observability_source = include_str!("../../observability.rs");
    let heartbeat_source = observability_source
        .split("pub(super) fn spawn_heartbeat_tasks")
        .nth(1)
        .expect("observability runtime should contain heartbeat task spawning");

    assert!(
        heartbeat_source.contains("spawn_monitored"),
        "heartbeat loops should report unexpected exits and panics through observability"
    );
}

#[tokio::test]
async fn monitored_background_task_panics_are_reported_to_error_endpoints() {
    let (endpoint, received_body) = capture_one_http_body().await;
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: false,
        error_endpoints: vec![ObservabilityErrorEndpoint {
            kind: Some("custom_json".to_string()),
            url: Some(endpoint),
            ..ObservabilityErrorEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };
    let runtime = ObservabilityRuntime::from_config(&config, 0x3554_334E)
        .expect("runtime config")
        .expect("observability enabled");

    let handle = runtime.spawn_monitored("indexer_runtime", async {
        panic!("indexer runtime exploded");
    });

    handle
        .await
        .expect("monitored task should catch and report panic");
    let body = tokio::time::timeout(Duration::from_secs(5), received_body)
        .await
        .expect("observability endpoint should receive error report")
        .expect("body sender should stay alive");
    let payload: Value = serde_json::from_str(&body).expect("observability JSON payload");

    assert_eq!(payload["event"]["type"], "background_task_panic");
    let message = payload["event"]["message"]
        .as_str()
        .expect("error message should be a string");
    assert!(
        message.contains("indexer_runtime") && message.contains("indexer runtime exploded"),
        "background task panic report should name the task and panic payload: {message}"
    );
}

#[tokio::test]
async fn monitored_background_task_errors_are_reported_to_error_endpoints() {
    let (endpoint, received_body) = capture_one_http_body().await;
    let config = ObservabilitySection {
        enabled: true,
        capture_panics: false,
        error_endpoints: vec![ObservabilityErrorEndpoint {
            kind: Some("custom_json".to_string()),
            url: Some(endpoint),
            ..ObservabilityErrorEndpoint::default()
        }],
        ..ObservabilitySection::default()
    };
    let runtime = ObservabilityRuntime::from_config(&config, 0x3554_334E)
        .expect("runtime config")
        .expect("observability enabled");

    let handle = runtime.spawn_monitored_result("telemetry_metrics", async {
        Err::<(), _>(anyhow::anyhow!("metrics server exploded"))
    });

    handle
        .await
        .expect("monitored task should catch and report task error");
    let body = tokio::time::timeout(Duration::from_secs(5), received_body)
        .await
        .expect("observability endpoint should receive error report")
        .expect("body sender should stay alive");
    let payload: Value = serde_json::from_str(&body).expect("observability JSON payload");

    assert_eq!(payload["event"]["type"], "background_task_error");
    let message = payload["event"]["message"]
        .as_str()
        .expect("error message should be a string");
    assert!(
        message.contains("telemetry_metrics") && message.contains("metrics server exploded"),
        "background task error report should name the task and original error: {message}"
    );
}

async fn capture_one_http_body() -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local observability sink");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let (body_tx, body_rx) = oneshot::channel();

    tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept observability request");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            stream.readable().await.expect("request readable");
            match stream.try_read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    request.extend_from_slice(&chunk[..read]);
                    if request_has_complete_body(&request) {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(err) => panic!("read observability request: {err}"),
            }
        }

        let body = http_body(&request);
        let _ = body_tx.send(body);
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
        let mut written = 0;
        while written < response.len() {
            stream.writable().await.expect("response writable");
            match stream.try_write(&response[written..]) {
                Ok(0) => break,
                Ok(bytes) => written += bytes,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(err) => panic!("write observability response: {err}"),
            }
        }
    });

    (endpoint, body_rx)
}

fn request_has_complete_body(request: &[u8]) -> bool {
    let Some(header_end) = find_header_end(request) else {
        return false;
    };
    let content_length = content_length(request).unwrap_or(0);
    request.len().saturating_sub(header_end) >= content_length
}

fn http_body(request: &[u8]) -> String {
    let header_end = find_header_end(request).expect("HTTP headers should be complete");
    String::from_utf8(request[header_end..].to_vec()).expect("HTTP body should be UTF-8 JSON")
}

fn find_header_end(request: &[u8]) -> Option<usize> {
    request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn content_length(request: &[u8]) -> Option<usize> {
    let header_end = find_header_end(request)?;
    let headers = String::from_utf8_lossy(&request[..header_end]);
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}
