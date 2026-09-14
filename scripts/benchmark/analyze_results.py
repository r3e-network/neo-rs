#!/usr/bin/env python3
"""Analyze Neo-N3 benchmark results from collected metrics.

This script processes benchmark data collected by collect_metrics.py and generates
comprehensive performance analysis reports.

Usage:
    python scripts/benchmark/analyze_results.py [output_file]
    
Args:
    output_file: Path to metrics_history.json (default: logs/benchmark/metrics_history.json)

Features:
    - Calculate TPS statistics (average, peak, stddev)
    - Analyze sync progress and block processing rates
    - Check optimization instrumentation status
    - Generate formatted text report
    - Export JSON summary for further processing
"""

import json
import statistics
import sys
from datetime import datetime
from pathlib import Path


def load_samples(filepath):
    """Load benchmark samples from JSON file."""
    with open(filepath, "r", encoding="utf-8") as f:
        return json.load(f)


def calculate_tps_metrics(samples):
    """Calculate transaction throughput statistics from samples.
    
    Args:
        samples: List of metric samples from benchmark
        
    Returns:
        dict: Statistics including avg, peak, min, stddev TPS
    """
    tps_values = []
    heights = []
    
    for i, sample in enumerate(samples[1:], 1):
        prev_sample = samples[i - 1]
        
        # Get height changes
        curr_height = sample["blockchain"]["height"]
        prev_height = prev_sample["blockchain"]["height"]
        
        if curr_height > prev_height:
            # Calculate time delta
            curr_ts = datetime.fromisoformat(sample["timestamp"])
            prev_ts = datetime.fromisoformat(prev_sample["timestamp"])
            time_delta = (curr_ts - prev_ts).total_seconds()
            
            # Avoid division by zero
            if time_delta <= 0:
                continue
            
            heights.append(curr_height)
            
            # Extract transaction counts if available
            curr_tx_samples = sample["prometheus"].get("neo_transactions_processed_total", [])
            prev_tx_samples = prev_sample["prometheus"].get("neo_transactions_processed_total", [])
            
            curr_tx = sum(m["value"] for m in curr_tx_samples) if curr_tx_samples else 0
            prev_tx = sum(m["value"] for m in prev_tx_samples) if prev_tx_samples else 0
            
            tx_delta = curr_tx - prev_tx
            
            if time_delta > 0 and tx_delta >= 0:
                tps = tx_delta / time_delta
                tps_values.append(tps)
    
    if not tps_values:
        return {
            "avg_tps": 0,
            "peak_tps": 0,
            "min_tps": 0,
            "stddev_tps": 0,
            "total_blocks": 0,
            "sample_count": len(samples)
        }
    
    return {
        "avg_tps": statistics.mean(tps_values),
        "peak_tps": max(tps_values),
        "min_tps": min(tps_values),
        "stddev_tps": statistics.stdev(tps_values) if len(tps_values) > 1 else 0,
        "total_blocks": max(heights) - min(heights) if heights else 0,
        "sample_count": len(samples),
        "tps_values": tps_values,
        "height_range": (min(heights), max(heights)) if heights else (0, 0)
    }


def check_optimization_instrumentation(samples):
    """Check which optimizations have proper instrumentation.
    
    Args:
        samples: Latest sample from benchmark
        
    Returns:
        dict: Optimization name mapped to availability status
    """
    latest_prom = samples[-1]["prometheus"] if samples else {}
    
    optimizations = {
        "Cuckoo Hash Lookup Latency": ["neo_syscall_lookup_latency_seconds", "syscall_lookup_duration"],
        "Batch Signature Verification": ["neo_signature_verification_duration_seconds", "ecdsa_verify_time"],
        "SIMD BLAKE2b Hash Rate": ["neo_blake2b_hash_rate_bytes_per_second", "blake2b_throughput"],
        "Prefetch Pipeline Utilization": ["neo_prefetch_pipeline_utilization_ratio", "pipeline_stage_latency"],
        "RocksDB Cache Efficiency": ["rocksdb_cache_hit_rate", "block_cache_efficiency"],
        "LRU Cache Performance": ["mpt_cache_hit_rate", "lru_hit_ratio"]
    }
    
    results = {}
    for opt_name, expected_metrics in optimizations.items():
        found = any(
            any(metric.lower() in key.lower() 
                for key in latest_prom.keys())
            for metric in expected_metrics
        )
        results[opt_name] = {
            "status": "✅ INSTRUMENTED" if found else "⚠️ NOT AVAILABLE",
            "expected_metrics": expected_metrics,
            "found_any": found
        }
    
    return results


def generate_report(metrics_stats, instrumented):
    """Generate formatted text report."""
    print("\n" + "="*78)
    print(" " * 25 + "NEO-N3 PERFORMANCE BENCHMARK REPORT")
    print(" " * 30 + f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("="*78)
    
    print(f"\n{'METRIC':<50} {'VALUE':>25}")
    print("-"*78)
    print(f"{'Total Samples Collected':<50} {metrics_stats['sample_count']:>25,}")
    print(f"{'Blocks Processed During Benchmark':<50} {metrics_stats['total_blocks']:>25,}")
    print(f"{'Height Range':<50} [{metrics_stats['height_range'][0]}, {metrics_stats['height_range'][1]}]")
    print()
    print(f"{'Throughput Metrics':<50} {'':>25}")
    print(f"  Average TPS:                          {metrics_stats['avg_tps']:>25,.2f}")
    print(f"  Peak TPS:                             {metrics_stats['peak_tps']:>25,.2f}")
    print(f"  Minimum TPS:                          {metrics_stats['min_tps']:>25,.2f}")
    print(f"  Standard Deviation:                   {metrics_stats['stddev_tps']:>25,.2f}")
    
    # Coefficient of variation (normalized stddev)
    if metrics_stats['avg_tps'] > 0:
        cv = (metrics_stats['stddev_tps'] / metrics_stats['avg_tps']) * 100
        print(f"  Variability (CV):                     {cv:>24.2f}%")
    
    print("\n" + "-"*78)
    print("  OPTIMIZATION METRICS STATUS")
    print("-"*78)
    
    for opt_name, info in instrumented.items():
        print(f"  {opt_name:<50} {info['status']:>25}")
    
    # Summary section
    print("\n" + "="*78)
    print("  SUMMARY & RECOMMENDATIONS")
    print("="*78)
    
    if metrics_stats['avg_tps'] > 0:
        print(f"\n✓ Throughput: Node processed blocks at average {metrics_stats['avg_tps']:.2f} TPS")
        print(f"  Peak performance reached {metrics_stats['peak_tps']:.2f} TPS")
        
        if metrics_stats['stddev_tps'] < metrics_stats['avg_tps'] * 0.3:
            print(f"  ✓ Stable performance (low variability)")
        else:
            print(f"  ⚠ Higher than normal variability detected")
    
    # Optimization coverage check
    optimized_count = sum(1 for v in instrumented.values() if v["found_any"])
    total_opts = len(instrumented)
    
    print(f"\n✓ Optimization Instrumentation: {optimized_count}/{total_opts} active")
    
    if optimized_count < total_opts * 0.5:
        print("\n⚠ PARTIAL COVERAGE WARNING:")
        print("  Several key optimizations lack detailed metrics instrumentation.")
        print("  Consider adding counters to neo-telemetry for complete visibility.")
    
    print("\n" + "="*78)
    print("  END OF REPORT")
    print("="*78 + "\n")


def export_json_summary(samples, filepath=None):
    """Export analysis results as JSON for downstream tools."""
    metrics_stats = calculate_tps_metrics(samples)
    instrumented = check_optimization_instrumentation(samples)
    
    summary = {
        "generated_at": datetime.now().isoformat(),
        "sample_count": len(samples),
        "throughput": metrics_stats,
        "optimizations": {k: v["status"] for k, v in instrumented.items()},
        "last_block_height": samples[-1]["blockchain"]["height"] if samples else 0,
        "last_peer_count": samples[-1]["blockchain"]["peers"] if samples else 0
    }
    
    if filepath is None:
        output_dir = Path("logs/benchmark")
        output_dir.mkdir(parents=True, exist_ok=True)
        filepath = output_dir / "benchmark_summary.json"
    
    with open(filepath, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2)
    
    return filepath, summary


def main():
    """Main entry point."""
    # Parse arguments
    if len(sys.argv) > 1:
        input_file = sys.argv[1]
    else:
        input_file = "logs/benchmark/metrics_history.json"
    
    # Validate input file exists
    if not Path(input_file).exists():
        print(f"[ERROR] Input file not found: {input_file}", file=sys.stderr)
        print("[INFO] Run collect_metrics.py first to gather benchmark data", file=sys.stderr)
        sys.exit(1)
    
    print(f"Loading benchmark data from: {input_file}")
    
    try:
        samples = load_samples(input_file)
    except json.JSONDecodeError as e:
        print(f"[ERROR] Invalid JSON in input file: {e}", file=sys.stderr)
        sys.exit(1)
    
    if not samples:
        print("[ERROR] No samples collected", file=sys.stderr)
        sys.exit(1)
    
    # Perform analysis
    print("\nAnalyzing benchmark data...")
    
    metrics_stats = calculate_tps_metrics(samples)
    instrumented = check_optimization_instrumentation(samples)
    
    # Generate text report
    generate_report(metrics_stats, instrumented)
    
    # Export JSON summary
    json_path, summary = export_json_summary(samples)
    print(f"\n📄 JSON summary saved to: {json_path}")
    
    # Exit codes
    if metrics_stats['avg_tps'] == 0:
        print("\n⚠ WARNING: No throughput data available - node may not be processing transactions")
        sys.exit(2)
    
    sys.exit(0)


if __name__ == "__main__":
    main()
