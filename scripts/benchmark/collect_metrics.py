#!/usr/bin/env python3
"""Neo-N3 Performance Metrics Collector - Collects real-world benchmark data.

This script collects comprehensive performance metrics from the optimized Neo-N3 node
running on testnet with Phase 1-3 optimizations enabled.

Usage:
    python scripts/benchmark/collect_metrics.py --duration 86400   # 24 hours
    python scripts/benchmark/collect_metrics.py --duration 3600    # 1 hour
    python scripts/benchmark/collect_metrics.py --duration 60      # Quick check

Features:
    - Real-time blockchain state monitoring via RPC
    - Prometheus metrics scraping
    - Continuous sampling at configurable intervals
    - JSON output for downstream analysis
    - Progress reporting during collection
"""

import requests
import json
import time
from datetime import datetime
from pathlib import Path
import argparse
import sys


# Configuration
METRICS_URL = "http://localhost:9090/metrics"
RPC_URL = "http://localhost:20332"
DEFAULT_DURATION = 86400  # 24 hours in seconds
DEFAULT_INTERVAL = 10     # Seconds between samples


def fetch_blockchain_state():
    """Get current blockchain state via Neo RPC methods.
    
    Returns:
        dict: Contains height, peer_count, mempool_size
    """
    result = {"height": 0, "peers": 0, "mempool_size": 0}
    
    try:
        # Get current block height
        resp = requests.post(RPC_URL, json={
            "jsonrpc": "2.0", 
            "id": 1, 
            "method": "getblockcount", 
            "params": []
        }, timeout=10)
        
        if resp.status_code == 200:
            data = resp.json()
            if "result" in data:
                result["height"] = data["result"]
        
        # Get network state including peer count
        resp = requests.post(RPC_URL, json={
            "jsonrpc": "2.0", 
            "id": 1, 
            "method": "getpeers", 
            "params": []
        }, timeout=10)
        
        if resp.status_code == 200:
            data = resp.json()
            if "result" in data:
                peers_result = data.get("result", {})
                if isinstance(peers_result, dict):
                    result["peers"] = len(peers_result.get("good_peers", []))
                elif isinstance(peers_result, list):
                    result["peers"] = len(peers_result)
        
        # Get mempool size (optional, may not be available in all versions)
        try:
            resp = requests.post(RPC_URL, json={
                "jsonrpc": "2.0", 
                "id": 1, 
                "method": "getrawmempool", 
                "params": []
            }, timeout=10)
            
            if resp.status_code == 200:
                data = resp.json()
                if "result" in data:
                    result["mempool_size"] = len(data["result"]) if isinstance(data["result"], list) else 0
        except Exception:
            # Mempool method may not exist in this version
            result["mempool_size"] = -1
            
    except requests.RequestException as e:
        print(f"[WARN] Failed to fetch blockchain state: {e}", file=sys.stderr)
    
    return result


def parse_prometheus_metrics(text):
    """Parse Prometheus text format metrics to structured dict.
    
    Args:
        text: Raw Prometheus metrics text output
        
    Returns:
        dict: Metric names mapped to lists of sample values
    """
    metrics = {}
    
    for line in text.split('\n'):
        line = line.strip()
        
        # Skip comments and empty lines
        if not line or line.startswith('#'):
            continue
        
        # Handle metric without labels (simple name value pair)
        parts = line.split()
        if len(parts) >= 2:
            try:
                name = parts[0]
                value = float(parts[1])
                
                if name not in metrics:
                    metrics[name] = []
                
                metrics[name].append({"value": value, "timestamp": datetime.now().isoformat()})
            except ValueError:
                # Not a numeric metric, skip
                continue
    
    return metrics


def collect_sample():
    """Collect single metrics sample from all sources.
    
    Returns:
        dict: Complete metrics snapshot including timestamp, prometheus metrics,
              and blockchain state
    """
    timestamp = datetime.now().isoformat()
    
    # Fetch Prometheus metrics
    prom_metrics = {}
    try:
        prom_resp = requests.get(METRICS_URL, timeout=5)
        if prom_resp.status_code == 200:
            prom_metrics = parse_prometheus_metrics(prom_resp.text)
    except requests.RequestException as e:
        print(f"[WARN] Prometheus scrape failed: {e}", file=sys.stderr)
    
    # Fetch blockchain state
    chain_state = fetch_blockchain_state()
    
    return {
        "timestamp": timestamp,
        "prometheus": prom_metrics,
        "blockchain": chain_state
    }


def run_benchmark(duration_seconds: int, interval_seconds: int = DEFAULT_INTERVAL):
    """Run continuous metrics collection for specified duration.
    
    Args:
        duration_seconds: Total benchmark duration in seconds
        interval_seconds: Time between samples in seconds
    """
    start_time = time.time()
    end_time = start_time + duration_seconds
    
    samples = []
    sample_count = 0
    
    print("="*70)
    print("NEO-N3 PERFORMANCE BENCHMARK COLLECTOR")
    print("="*70)
    print(f"Target Duration: {duration_seconds}s ({duration_seconds/3600:.1f}h)")
    print(f"Sample Interval: {interval_seconds}s")
    print(f"Expected Samples: {duration_seconds // interval_seconds}")
    print(f"Start Time: {datetime.fromtimestamp(start_time).isoformat()}")
    print("="*70)
    print()
    
    while time.time() < end_time:
        sample = collect_sample()
        samples.append(sample)
        sample_count += 1
        
        # Extract key metrics for progress display
        height = sample["blockchain"]["height"]
        peers = sample["blockchain"]["peers"]
        mempool = sample["blockchain"]["mempool_size"]
        
        # Calculate TPS if we have transaction counter
        tps = 0.0
        tx_samples = sample["prometheus"].get("neo_transactions_processed_total", [])
        if tx_samples:
            total_tx = sum(m["value"] for m in tx_samples)
            elapsed = (datetime.now() - datetime.fromisoformat(samples[0]["timestamp"])).total_seconds()
            if elapsed > 0:
                tps = total_tx / elapsed
        
        # Progress indicator
        elapsed = time.time() - start_time
        remaining = end_time - time.time()
        eta = datetime.fromtimestamp(remaining + time.time()).strftime("%H:%M:%S")
        
        print(
            f"[{sample['timestamp']}] "
            f"H: {height:>8} | "
            f"P: {peers:3d} | "
            f"M: {mempool:6d} | "
            f"TX: {total_tx if 'total_tx' in dir() else 'N/A'} | "
            f"TPS: {tps:.2f} | "
            f"ETA: {eta}"
        )
        
        elapsed = time.time() - start_time
        
        if sample_count % 10 == 0 or sample_count == 1:
            avg_tps = sum(t[1]["blockchain"]["height"] - samples[t[0]-1][1]["blockchain"]["height"] 
                         for t in range(1, min(sample_count+1, len(samples))) 
                         for i, s in enumerate([samples])) / max(1, sample_count) if samples else 0
            
            print(f"\n[Avg TPS over {sample_count} samples]: {avg_tps:.2f} TX/s")
        
        # Wait before next sample
        time.sleep(interval_seconds)
    
    # Save results
    output_dir = Path("logs/benchmark")
    output_dir.mkdir(parents=True, exist_ok=True)
    
    output_file = output_dir / "metrics_history.json"
    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(samples, f, indent=2)
    
    # Print summary
    print("\n" + "="*70)
    print("BENCHMARK COMPLETE")
    print("="*70)
    print(f"Total Samples Collected: {len(samples)}")
    print(f"Duration: {time.time() - start_time:.1f}s ({(time.time() - start_time)/3600:.1f}h)")
    print(f"Output File: {output_file.absolute()}")
    print("="*70)
    
    return samples


def main():
    """Main entry point with argument parsing."""
    parser = argparse.ArgumentParser(
        description="Collect Neo-N3 performance metrics from live testnet node",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python collect_metrics.py --duration 86400      # Run for 24 hours
  python collect_metrics.py --duration 3600       # Run for 1 hour  
  python collect_metrics.py --duration 60         # Quick 1-minute check
  python collect_metrics.py --duration 3600 --interval 60  # Less frequent sampling
        """
    )
    
    parser.add_argument(
        "--duration", "-d",
        type=int,
        default=DEFAULT_DURATION,
        help=f"Benchmark duration in seconds (default: {DEFAULT_DURATION}, 24 hours)"
    )
    
    parser.add_argument(
        "--interval", "-i",
        type=int,
        default=DEFAULT_INTERVAL,
        help=f"Sample interval in seconds (default: {DEFAULT_INTERVAL})"
    )
    
    args = parser.parse_args()
    
    if args.duration < 60:
        print("[ERROR] Minimum duration is 60 seconds", file=sys.stderr)
        sys.exit(1)
    
    if args.interval < 5:
        print("[ERROR] Minimum interval is 5 seconds", file=sys.stderr)
        sys.exit(1)
    
    # Validate that node is running
    print("[INFO] Validating node connectivity...")
    try:
        state = fetch_blockchain_state()
        if state["height"] == 0:
            print("[WARN] Node may not be synced yet (height=0)")
        else:
            print(f"[INFO] Node connected: Height {state['height']}, Peers {state['peers']}")
    except Exception as e:
        print(f"[ERROR] Cannot connect to node: {e}", file=sys.stderr)
        print("[INFO] Make sure neo-node is running with:")
        print("  cargo run --release --bin neo-node -- config/testnet-production.toml")
        sys.exit(1)
    
    print()
    run_benchmark(args.duration, args.interval)


if __name__ == "__main__":
    main()
