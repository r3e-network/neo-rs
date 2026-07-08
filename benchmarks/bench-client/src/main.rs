//! # bench-client
//!
//! Benchmark client binary for driving local node performance probes.
//!
//! ## Boundary
//!
//! This binary drives benchmarks against lower crates and must not become
//! production node orchestration.
//!
//! ## Contents
//!
//! - `bench-client`: bench-client entrypoint API.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use serde_json::{json, Value};

#[derive(Parser, Debug, Clone)]
#[command(about = "JSON-RPC load generator for Neo N3 nodes (neo-rs / neo-cli / neo-go)")]
struct Args {
    /// Node JSON-RPC endpoint.
    #[arg(long, default_value = "http://127.0.0.1:10332")]
    url: String,
    /// Workload to run.
    #[arg(long, value_enum, default_value_t = Scenario::BlockRead)]
    scenario: Scenario,
    /// For `method` scenario: the RPC method name.
    #[arg(long, default_value = "getblockcount")]
    method: String,
    /// For `method` scenario: the params as a JSON array.
    #[arg(long, default_value = "[]")]
    params: String,
    /// Upper bound for random block heights (block-read / header-read).
    #[arg(long, default_value_t = 100_000)]
    max_height: u64,
    /// Concurrent workers.
    #[arg(long, default_value_t = 32)]
    concurrency: usize,
    /// Measurement duration in seconds.
    #[arg(long, default_value_t = 20)]
    duration: u64,
    /// Warmup seconds (not counted).
    #[arg(long, default_value_t = 3)]
    warmup: u64,
    /// Label for this run (e.g. the implementation name) in the output.
    #[arg(long, default_value = "node")]
    label: String,
    /// Emit a JSON results line as well as the human table.
    #[arg(long)]
    json: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    /// getblockcount — cheapest call, measures pure RPC overhead.
    Count,
    /// getblock at a random height (verbose) — read + serialize a full block.
    BlockRead,
    /// getblockheader at a random height (verbose).
    HeaderRead,
    /// A fixed --method/--params call.
    Method,
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let args = Args::parse();
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(args.concurrency * 2)
        .timeout(Duration::from_secs(30))
        .build()?;

    println!(
        "neo-bench: {} | {:?} | {} workers | {}s (+{}s warmup) | {}",
        args.label, args.scenario, args.concurrency, args.duration, args.warmup, args.url
    );

    let stop = Arc::new(AtomicBool::new(false));
    let counting = Arc::new(AtomicBool::new(false));
    let ok = Arc::new(AtomicU64::new(0));
    let err = Arc::new(AtomicU64::new(0));

    // Simple xorshift RNG seed per worker (no rand dep).
    let mut handles = Vec::new();
    for w in 0..args.concurrency {
        let (client, args, stop, counting, ok, err) = (
            client.clone(),
            args.clone(),
            Arc::clone(&stop),
            Arc::clone(&counting),
            Arc::clone(&ok),
            Arc::clone(&err),
        );
        handles.push(tokio::spawn(async move {
            let mut latencies: Vec<u64> = Vec::with_capacity(100_000);
            let mut rng: u64 = 0x9E37_79B9_7F4A_7C15 ^ ((w as u64).wrapping_mul(0x1000_0000_1B3));
            while !stop.load(Ordering::Relaxed) {
                let body = build_request(&args, &mut rng);
                let started = Instant::now();
                let res = client.post(&args.url).json(&body).send().await;
                let elapsed = started.elapsed().as_micros() as u64;
                let success = match res {
                    Ok(r) => {
                        r.status().is_success()
                            && r.json::<Value>()
                                .await
                                .map(|v| v.get("error").map(Value::is_null).unwrap_or(true))
                                .unwrap_or(false)
                    }
                    Err(_) => false,
                };
                if counting.load(Ordering::Relaxed) {
                    if success {
                        ok.fetch_add(1, Ordering::Relaxed);
                        latencies.push(elapsed);
                    } else {
                        err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            latencies
        }));
    }

    // Warmup, then measure.
    tokio::time::sleep(Duration::from_secs(args.warmup)).await;
    counting.store(true, Ordering::Relaxed);
    let measure_start = Instant::now();
    tokio::time::sleep(Duration::from_secs(args.duration)).await;
    stop.store(true, Ordering::Relaxed);
    let wall = measure_start.elapsed().as_secs_f64();

    let mut all: Vec<u64> = Vec::new();
    for h in handles {
        if let Ok(mut l) = h.await {
            all.append(&mut l);
        }
    }
    all.sort_unstable();

    let ok = ok.load(Ordering::Relaxed);
    let err = err.load(Ordering::Relaxed);
    let rps = ok as f64 / wall;
    let pct = |p: f64| -> f64 {
        if all.is_empty() {
            return 0.0;
        }
        let idx = ((p / 100.0) * (all.len() as f64 - 1.0)).round() as usize;
        all[idx.min(all.len() - 1)] as f64 / 1000.0 // ms
    };

    println!("─────────────────────────────────────────────");
    println!("  requests ok : {ok}");
    println!("  errors      : {err}");
    println!("  throughput  : {rps:.0} req/s");
    println!("  latency p50 : {:.2} ms", pct(50.0));
    println!("  latency p95 : {:.2} ms", pct(95.0));
    println!("  latency p99 : {:.2} ms", pct(99.0));
    println!("  latency max : {:.2} ms", pct(100.0));
    println!("─────────────────────────────────────────────");

    if args.json {
        let out = json!({
            "label": args.label,
            "scenario": format!("{:?}", args.scenario),
            "concurrency": args.concurrency,
            "duration_s": args.duration,
            "requests_ok": ok,
            "errors": err,
            "rps": rps,
            "p50_ms": pct(50.0),
            "p95_ms": pct(95.0),
            "p99_ms": pct(99.0),
            "max_ms": pct(100.0),
        });
        println!("{out}");
    }
    Ok(())
}

fn next_rand(rng: &mut u64) -> u64 {
    // xorshift64*
    let mut x = *rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *rng = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn build_request(args: &Args, rng: &mut u64) -> Value {
    let (method, params) = match args.scenario {
        Scenario::Count => ("getblockcount", json!([])),
        Scenario::BlockRead => {
            let h = next_rand(rng) % args.max_height.max(1);
            ("getblock", json!([h, true]))
        }
        Scenario::HeaderRead => {
            let h = next_rand(rng) % args.max_height.max(1);
            ("getblockheader", json!([h, true]))
        }
        Scenario::Method => {
            let params: Value = serde_json::from_str(&args.params).unwrap_or(json!([]));
            (args.method.as_str(), params)
        }
    };
    json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
}
