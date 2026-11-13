//! Live state-root references for validation beyond the static JSONL range.

use anyhow::{Context, Result, bail};
use neo_primitives::UInt256;
use neo_system::NeoSystem;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tracing::info;

pub(super) fn configure_live_reference_roots(system: &NeoSystem) -> Result<()> {
    let Ok(configured) = std::env::var("NEO_REFERENCE_ROOTS_RPC") else {
        return Ok(());
    };
    let endpoints: Arc<Vec<String>> = Arc::new(
        configured
            .split(',')
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    );
    if endpoints.is_empty() {
        bail!("NEO_REFERENCE_ROOTS_RPC must contain at least one RPC endpoint");
    }

    let state_store = system
        .state_store()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .context("NEO_REFERENCE_ROOTS_RPC requires the state service to be enabled")?;
    let fetch_endpoints = Arc::clone(&endpoints);
    let cache = Arc::new(LiveReferenceRootCache::new(Arc::new(
        move |start, count| fetch_reference_root_batch(&fetch_endpoints, start, count),
    )));
    state_store.set_reference_root_fetcher(Arc::new(move |height| cache.get(height).ok()));
    info!(
        target: "neo::state_service",
        endpoints = ?endpoints,
        prefetch = REFERENCE_ROOT_PREFETCH,
        "live reference state-root validation enabled beyond the static range"
    );
    Ok(())
}

const REFERENCE_ROOT_PREFETCH: u32 = 32;
const REFERENCE_ROOT_ATTEMPTS: usize = 6;
const REFERENCE_ROOT_RETRY_BACKOFF_MS: [u64; REFERENCE_ROOT_ATTEMPTS - 1] =
    [100, 200, 500, 1_000, 2_000];
type ReferenceRootResult = Result<UInt256, String>;
type ReferenceRootBatch = Result<HashMap<u32, ReferenceRootResult>, String>;

struct LiveReferenceRootCache {
    fetch_batch: Arc<dyn Fn(u32, u32) -> ReferenceRootBatch + Send + Sync>,
    roots: Mutex<HashMap<u32, ReferenceRootResult>>,
    batch_lock: Mutex<()>,
}

impl LiveReferenceRootCache {
    fn new(fetch_batch: Arc<dyn Fn(u32, u32) -> ReferenceRootBatch + Send + Sync>) -> Self {
        Self {
            fetch_batch,
            roots: Mutex::new(HashMap::new()),
            batch_lock: Mutex::new(()),
        }
    }

    fn get(&self, height: u32) -> ReferenceRootResult {
        if let Some(result) = self
            .roots
            .lock()
            .map_err(|_| "reference-root cache lock poisoned".to_string())?
            .remove(&height)
        {
            return result;
        }

        let _batch_guard = self
            .batch_lock
            .lock()
            .map_err(|_| "reference-root batch lock poisoned".to_string())?;
        if let Some(result) = self
            .roots
            .lock()
            .map_err(|_| "reference-root cache lock poisoned".to_string())?
            .remove(&height)
        {
            return result;
        }

        let fetched = (self.fetch_batch)(height, REFERENCE_ROOT_PREFETCH)?;
        let mut roots = self
            .roots
            .lock()
            .map_err(|_| "reference-root cache lock poisoned".to_string())?;
        roots.extend(fetched);
        roots
            .remove(&height)
            .unwrap_or_else(|| Err(format!("reference-root prefetch omitted height {height}")))
    }
}

fn fetch_reference_root_batch(
    endpoints: &Arc<Vec<String>>,
    start_height: u32,
    count: u32,
) -> ReferenceRootBatch {
    // Cap every prefetch at the remote StateService's published local root. Near the
    // chain tip, asking for all 32 heights would otherwise include future blocks and
    // fail the whole batch even though the requested root is already available.
    // Construct and drop the blocking client off async runtime threads.
    let endpoints = Arc::clone(endpoints);
    thread::Builder::new()
        .name(format!("state-root-rpc-batch-{start_height}"))
        .spawn(move || {
            let client = build_reference_root_client()?;
            let mut errors = Vec::new();
            for (attempt, backoff_ms) in REFERENCE_ROOT_RETRY_BACKOFF_MS
                .iter()
                .chain(std::iter::once(&0))
                .enumerate()
            {
                for endpoint in endpoints.iter() {
                    let result = fetch_available_reference_root_count(
                        &client,
                        endpoint,
                        start_height,
                        count,
                    )
                    .and_then(|available_count| {
                        let request = Value::Array(
                            (start_height..start_height.saturating_add(available_count))
                                .map(|height| {
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": height,
                                        "method": "getstateroot",
                                        "params": [height],
                                    })
                                })
                                .collect(),
                        );
                        client
                            .post(endpoint)
                            .json(&request)
                            .send()
                            .and_then(reqwest::blocking::Response::error_for_status)
                            .and_then(reqwest::blocking::Response::json::<Value>)
                            .map_err(|error| error.to_string())
                            .and_then(|payload| {
                                parse_reference_root_batch(
                                    payload,
                                    start_height,
                                    available_count,
                                )
                            })
                    });
                    match result {
                        Ok(roots) => {
                            for (height, root) in &roots {
                                if let Ok(root) = root {
                                    info!(
                                        target: "neo::state_service",
                                        height = *height,
                                        root_hash = %root,
                                        endpoint,
                                        attempt = attempt + 1,
                                        "reference state root fetched from live RPC"
                                    );
                                }
                            }
                            return Ok(roots);
                        }
                        Err(error) => {
                            errors.push(format!("attempt {} {endpoint}: {error}", attempt + 1))
                        }
                    }
                }
                if attempt + 1 < REFERENCE_ROOT_ATTEMPTS {
                    thread::sleep(Duration::from_millis(
                        *backoff_ms,
                    ));
                }
            }
            Err(format!(
                "all reference RPC endpoints failed for batch {start_height}..{} after {REFERENCE_ROOT_ATTEMPTS} attempts: {}",
                start_height.saturating_add(count),
                errors.join("; ")
            ))
        })
        .map_err(|error| format!("failed to spawn reference RPC batch thread: {error}"))?
        .join()
        .map_err(|_| "reference RPC batch thread panicked".to_string())?
}

fn fetch_available_reference_root_count(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    start_height: u32,
    count: u32,
) -> Result<u32, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getstateheight",
        "params": [],
    });
    let payload = client
        .post(endpoint)
        .json(&request)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::json::<Value>)
        .map_err(|error| error.to_string())?;
    parse_available_reference_root_count(payload, start_height, count)
}

fn parse_available_reference_root_count(
    payload: Value,
    start_height: u32,
    count: u32,
) -> Result<u32, String> {
    if let Some(error) = payload.get("error") {
        return Err(format!("RPC returned an error: {error}"));
    }
    let local_root_index = payload
        .get("result")
        .and_then(|result| result.get("localrootindex"))
        .and_then(Value::as_u64)
        .ok_or_else(|| "RPC state-height response has no valid localrootindex".to_string())?;
    if local_root_index < u64::from(start_height) {
        return Err(format!(
            "reference state root {start_height} is not yet published (localrootindex {local_root_index})"
        ));
    }
    let available = local_root_index - u64::from(start_height) + 1;
    Ok(u32::try_from(available.min(u64::from(count))).unwrap_or(count))
}

fn parse_reference_root_batch(payload: Value, start_height: u32, count: u32) -> ReferenceRootBatch {
    let responses = payload
        .as_array()
        .ok_or_else(|| "RPC batch response is not an array".to_string())?;
    let mut roots = HashMap::with_capacity(count as usize);
    for response in responses {
        let height = response
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| "RPC batch response has no valid id".to_string())?;
        let height =
            u32::try_from(height).map_err(|_| "RPC batch response id exceeds u32".to_string())?;
        if height < start_height || height >= start_height.saturating_add(count) {
            return Err(format!(
                "RPC batch response contains unexpected height {height}"
            ));
        }
        if roots.contains_key(&height) {
            return Err(format!(
                "RPC batch response contains duplicate height {height}"
            ));
        }
        let root = parse_reference_root(response.clone(), height)
            .map_err(|error| format!("height {height}: {error}"))?;
        roots.insert(height, Ok(root));
    }
    if roots.len() != count as usize {
        return Err(format!(
            "RPC batch response returned {} of {count} requested roots",
            roots.len()
        ));
    }
    Ok(roots)
}

fn build_reference_root_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("failed to construct RPC client: {error}"))
}

#[cfg(test)]
fn fetch_reference_root(endpoints: &Arc<Vec<String>>, height: u32) -> Result<UInt256, String> {
    // reqwest::blocking owns a runtime. Keep its construction and destruction off
    // the node's async runtime threads, including when validation aborts startup.
    let endpoints = Arc::clone(endpoints);
    thread::Builder::new()
        .name(format!("state-root-rpc-{height}"))
        .spawn(move || fetch_reference_root_blocking(&endpoints, height))
        .map_err(|error| format!("failed to spawn reference RPC thread: {error}"))?
        .join()
        .map_err(|_| "reference RPC thread panicked".to_string())?
}

#[cfg(test)]
fn fetch_reference_root_blocking(endpoints: &[String], height: u32) -> Result<UInt256, String> {
    let client = build_reference_root_client()?;
    fetch_reference_root_with_client(&client, endpoints, height)
}

#[cfg(test)]
fn fetch_reference_root_with_client(
    client: &reqwest::blocking::Client,
    endpoints: &[String],
    height: u32,
) -> Result<UInt256, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getstateroot",
        "params": [height],
    });
    let mut errors = Vec::new();

    for (attempt, backoff_ms) in REFERENCE_ROOT_RETRY_BACKOFF_MS
        .iter()
        .chain(std::iter::once(&0))
        .enumerate()
    {
        for endpoint in endpoints {
            let result = client
                .post(endpoint)
                .json(&request)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(reqwest::blocking::Response::json::<Value>)
                .map_err(|error| error.to_string())
                .and_then(|payload| parse_reference_root(payload, height));
            match result {
                Ok(root) => {
                    info!(
                        target: "neo::state_service",
                        height,
                        root_hash = %root,
                        endpoint,
                        attempt = attempt + 1,
                        "reference state root fetched from live RPC"
                    );
                    return Ok(root);
                }
                Err(error) => errors.push(format!("attempt {} {endpoint}: {error}", attempt + 1)),
            }
        }
        if attempt + 1 < REFERENCE_ROOT_ATTEMPTS {
            // A bounded backoff absorbs transient seed/gateway failures without
            // weakening validation: every exhausted attempt still returns Err.
            thread::sleep(Duration::from_millis(*backoff_ms));
        }
    }

    Err(format!(
        "all reference RPC endpoints failed for height {height} after {REFERENCE_ROOT_ATTEMPTS} attempts: {}",
        errors.join("; ")
    ))
}

fn parse_reference_root(payload: Value, height: u32) -> Result<UInt256, String> {
    if let Some(error) = payload.get("error") {
        return Err(format!("RPC returned an error: {error}"));
    }
    let result = payload
        .get("result")
        .filter(|result| !result.is_null())
        .ok_or_else(|| "RPC response has no state-root result".to_string())?;
    let index = result
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| "RPC state-root result has no valid index".to_string())?;
    if index != u64::from(height) {
        return Err(format!(
            "RPC state-root index mismatch: requested {height}, received {index}"
        ));
    }
    let root = result
        .get("roothash")
        .and_then(Value::as_str)
        .ok_or_else(|| "RPC state-root result has no valid roothash".to_string())?;
    UInt256::parse(root).map_err(|error| format!("invalid RPC state-root hash '{root}': {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reference_root_and_checks_height() {
        let root = UInt256::parse(&format!("0x{}", "11".repeat(32))).unwrap();
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"version": 0, "index": 42, "roothash": root.to_string()}
        });
        assert_eq!(parse_reference_root(payload.clone(), 42), Ok(root));
        assert!(parse_reference_root(payload, 41).is_err());
    }

    #[test]
    fn rejects_rpc_errors_and_missing_results() {
        assert!(parse_reference_root(json!({"error": {"code": -1}}), 1).is_err());
        assert!(parse_reference_root(json!({"result": null}), 1).is_err());
    }

    #[test]
    fn state_height_caps_prefetch_and_defers_unpublished_roots() {
        let payload = |localrootindex| json!({"result": {"localrootindex": localrootindex, "validatedrootindex": localrootindex}});
        assert_eq!(
            parse_available_reference_root_count(payload(150), 100, 32),
            Ok(32)
        );
        assert_eq!(
            parse_available_reference_root_count(payload(104), 100, 32),
            Ok(5)
        );
        assert_eq!(
            parse_available_reference_root_count(payload(100), 100, 32),
            Ok(1)
        );
        assert!(parse_available_reference_root_count(payload(99), 100, 32).is_err());
        assert!(parse_available_reference_root_count(json!({"result": {}}), 100, 32).is_err());
        assert!(parse_available_reference_root_count(json!({"error": {}}), 100, 32).is_err());
    }

    #[test]
    fn prefetch_reuses_every_cached_root_and_fails_closed() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let fetch_calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = fetch_calls.clone();
        let cache = LiveReferenceRootCache::new(Arc::new(move |start, count| {
            observed_calls.fetch_add(1, Ordering::SeqCst);
            Ok((start..start + count)
                .map(|height| (height, Ok(UInt256::from([height as u8; 32]))))
                .collect())
        }));

        for height in 10..42 {
            assert_eq!(cache.get(height), Ok(UInt256::from([height as u8; 32])));
        }
        assert_eq!(fetch_calls.load(Ordering::SeqCst), 1);
        assert_eq!(cache.get(42), Ok(UInt256::from([42; 32])));
        assert_eq!(fetch_calls.load(Ordering::SeqCst), 2);

        let offline = LiveReferenceRootCache::new(Arc::new(|_, _| {
            Err("official RPC unavailable".to_string())
        }));
        assert!(offline.get(100).is_err());
    }

    #[test]
    fn batch_parser_accepts_reordered_responses_and_rejects_incomplete_or_duplicate_data() {
        let response = |height: u32| {
            json!({
                "jsonrpc": "2.0",
                "id": height,
                "result": {
                    "version": 0,
                    "index": height,
                    "roothash": UInt256::from([height as u8; 32]).to_string()
                }
            })
        };
        let parsed =
            parse_reference_root_batch(json!([response(11), response(10), response(12)]), 10, 3)
                .expect("reordered batch");
        assert_eq!(parsed.get(&10), Some(&Ok(UInt256::from([10; 32]))));
        assert_eq!(parsed.get(&12), Some(&Ok(UInt256::from([12; 32]))));
        assert!(parse_reference_root_batch(json!([response(10), response(11)]), 10, 3).is_err());
        assert!(
            parse_reference_root_batch(json!([response(10), response(10), response(12)]), 10, 3)
                .is_err()
        );
        assert!(parse_reference_root_batch(json!([response(10), response(13)]), 10, 2).is_err());
        assert!(parse_reference_root_batch(json!({"error": "batch failed"}), 10, 2).is_err());
    }

    #[test]
    fn retries_transient_http_failures_without_changing_the_expected_root() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC server");
        let address = listener.local_addr().expect("test RPC address");
        let root = UInt256::from([0x42; 32]);
        let server = thread::spawn(move || {
            for attempt in 0..3 {
                let (mut stream, _) = listener.accept().expect("accept RPC request");
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request);
                let (status, body) = if attempt < 2 {
                    ("502 Bad Gateway", "temporary gateway failure".to_string())
                } else {
                    (
                        "200 OK",
                        json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "result": {"version": 0, "index": 42, "roothash": root.to_string()}
                        })
                        .to_string(),
                    )
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("write RPC response");
            }
        });

        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .expect("direct loopback RPC client");
        let endpoints = vec![format!("http://{address}")];
        assert_eq!(
            fetch_reference_root_with_client(&client, &endpoints, 42),
            Ok(root)
        );
        server.join().expect("test RPC server");
    }

    #[test]
    #[ignore = "requires the official MainNet RPC endpoint"]
    fn fetches_official_mainnet_reference_root() {
        let endpoints = Arc::new(vec!["http://seed1.neo.org:10332".to_string()]);
        let root = fetch_reference_root(&endpoints, 13_150_241).expect("official reference root");
        assert_eq!(
            root.to_string(),
            "0xf40011ed0811bb84a3e57ada1e776683e057bf762252efb9f4e178eee5c3c55d"
        );
    }
}
