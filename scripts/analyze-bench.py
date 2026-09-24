#!/usr/bin/env python3
"""
Benchmark Analysis Script
Analyzes Criterion benchmark results and detects performance regressions
"""

import json
import sys
import argparse
from pathlib import Path
from typing import Dict, List, Tuple, Optional
from dataclasses import dataclass
from enum import Enum


class ChangeType(Enum):
    IMPROVEMENT = "improvement"
    REGRESSION = "regression"
    NO_CHANGE = "no_change"


@dataclass
class BenchmarkResult:
    name: str
    baseline_mean: float
    current_mean: float
    change_percent: float
    change_type: ChangeType
    confidence: float = 0.95


class BenchmarkAnalyzer:
    def __init__(self, baseline_dir: Path, current_dir: Path, threshold: float):
        self.baseline_dir = baseline_dir
        self.current_dir = current_dir
        self.threshold = threshold
        self.results: List[BenchmarkResult] = []

    def analyze(self) -> List[BenchmarkResult]:
        """Analyze all benchmark results."""
        # Find all change.json files in criterion output
        change_files = list(self.current_dir.rglob("change/estimates.json"))

        for change_file in change_files:
            result = self.analyze_benchmark(change_file)
            if result:
                self.results.append(result)

        return self.results

    def analyze_benchmark(self, change_file: Path) -> Optional[BenchmarkResult]:
        """Analyze a single benchmark result."""
        try:
            with open(change_file, 'r') as f:
                data = json.load(f)

            # Extract benchmark name from path
            # Path structure: target/criterion/<bench_name>/<group>/change/estimates.json
            parts = change_file.parts
            criterion_idx = parts.index('criterion')
            bench_name = '/'.join(parts[criterion_idx+1:-2])

            # Get mean time change
            mean_change = data.get('mean', {})
            point_estimate = mean_change.get('point_estimate', 0)

            # Convert to percentage
            change_percent = point_estimate * 100

            # Determine change type
            if abs(change_percent) < self.threshold:
                change_type = ChangeType.NO_CHANGE
            elif change_percent > 0:
                change_type = ChangeType.REGRESSION
            else:
                change_type = ChangeType.IMPROVEMENT

            # Get confidence interval
            confidence_interval = mean_change.get('confidence_interval', {})
            confidence = confidence_interval.get('confidence_level', 0.95)

            return BenchmarkResult(
                name=bench_name,
                baseline_mean=0,  # Not directly available in change file
                current_mean=0,   # Not directly available in change file
                change_percent=change_percent,
                change_type=change_type,
                confidence=confidence
            )
        except (json.JSONDecodeError, KeyError, ValueError) as e:
            print(f"Warning: Could not parse {change_file}: {e}", file=sys.stderr)
            return None

    def generate_report(self, output_file: Path):
        """Generate JSON report of analysis."""
        report = {
            "threshold_percent": self.threshold,
            "total_benchmarks": len(self.results),
            "regressions": [],
            "improvements": [],
            "no_change": []
        }

        for result in self.results:
            entry = {
                "name": result.name,
                "change_percent": round(result.change_percent, 2),
                "confidence": result.confidence
            }

            if result.change_type == ChangeType.REGRESSION:
                report["regressions"].append(entry)
            elif result.change_type == ChangeType.IMPROVEMENT:
                report["improvements"].append(entry)
            else:
                report["no_change"].append(entry)

        # Write report
        output_file.parent.mkdir(parents=True, exist_ok=True)
        with open(output_file, 'w') as f:
            json.dump(report, f, indent=2)

        # Print summary
        self.print_summary(report)

        # Exit with error if regressions found
        if report["regressions"]:
            return 1
        return 0

    def print_summary(self, report: Dict):
        """Print human-readable summary."""
        print("\n" + "="*70)
        print("BENCHMARK REGRESSION ANALYSIS")
        print("="*70)
        print(f"Threshold: {report['threshold_percent']}%")
        print(f"Total benchmarks: {report['total_benchmarks']}")
        print()

        if report["regressions"]:
            print(f"⚠️  REGRESSIONS DETECTED: {len(report['regressions'])}")
            print("-" * 70)
            for reg in sorted(report["regressions"],
                             key=lambda x: x["change_percent"],
                             reverse=True):
                print(f"  {reg['name']}")
                print(f"    Change: +{reg['change_percent']:.2f}% (slower)")
                print()

        if report["improvements"]:
            print(f"✓ IMPROVEMENTS: {len(report['improvements'])}")
            print("-" * 70)
            for imp in sorted(report["improvements"],
                             key=lambda x: x["change_percent"]):
                print(f"  {imp['name']}")
                print(f"    Change: {imp['change_percent']:.2f}% (faster)")
                print()

        if report["no_change"]:
            print(f"→ NO SIGNIFICANT CHANGE: {len(report['no_change'])}")

        print("="*70)


def main():
    parser = argparse.ArgumentParser(
        description="Analyze Criterion benchmark results for regressions"
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        required=True,
        help="Path to baseline benchmark results"
    )
    parser.add_argument(
        "--current",
        type=Path,
        required=True,
        help="Path to current benchmark results"
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=10.0,
        help="Regression threshold in percent (default: 10)"
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output file for JSON report"
    )

    args = parser.parse_args()

    # Validate paths
    if not args.current.exists():
        print(f"Error: Current results directory not found: {args.current}",
              file=sys.stderr)
        return 1

    # Analyze
    analyzer = BenchmarkAnalyzer(args.baseline, args.current, args.threshold)
    analyzer.analyze()

    # Generate report
    return analyzer.generate_report(args.output)


if __name__ == "__main__":
    sys.exit(main())
