#!/usr/bin/env python3
"""
Benchmark Analysis Script
Analyzes Criterion benchmark results and detects performance regressions

This script can run standalone if Python is not available - the shell script
will fall back to basic analysis.
"""

import json
import sys
import argparse
from pathlib import Path
from typing import Dict, List, Optional
from dataclasses import dataclass
from enum import Enum


class ChangeType(Enum):
    """Type of performance change detected."""
    IMPROVEMENT = "improvement"
    REGRESSION = "regression"
    NO_CHANGE = "no_change"


@dataclass
class BenchmarkResult:
    """Result of a single benchmark comparison."""
    name: str
    baseline_mean: float
    current_mean: float
    change_percent: float
    change_type: ChangeType
    confidence: float = 0.95
    absolute_delta_ns: float = 0.0


class BenchmarkAnalyzer:
    """Analyzes Criterion benchmark results for performance regressions."""

    def __init__(
        self,
        baseline_dir: Path,
        current_dir: Path,
        threshold: float,
        changed_since: Optional[float] = None,
        min_abs_ns: float = 0.0,
    ):
        self.baseline_dir = baseline_dir
        self.current_dir = current_dir
        self.threshold = threshold
        # Only consider comparisons written by the run that just finished.
        # Criterion leaves `change/` directories behind, so without this filter
        # a gate run would also report comparisons from earlier runs.
        self.changed_since = changed_since
        # A 15% change on a 1.4 us benchmark is 0.2 us: below the run-to-run
        # noise of the host and irrelevant to block throughput. Requiring a
        # minimum absolute effect keeps microbenchmarks from blocking merges.
        self.min_abs_ns = min_abs_ns
        self.results: List[BenchmarkResult] = []

    def analyze(self) -> List[BenchmarkResult]:
        """Analyze all benchmark results."""
        # Find all change/estimates.json files in criterion output
        change_files = list(self.current_dir.rglob("change/estimates.json"))

        if self.changed_since is not None:
            change_files = [
                path
                for path in change_files
                if path.stat().st_mtime >= self.changed_since
            ]

        if not change_files:
            print("Warning: No benchmark comparison results found", file=sys.stderr)
            print(f"Searched in: {self.current_dir}", file=sys.stderr)
            print("Make sure to run: ./scripts/bench-regression.sh --compare", file=sys.stderr)

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

            # Absolute effect size. criterion stores the current run's mean in
            # ns next to the comparison; the baseline mean follows from the
            # ratio, so no knowledge of the baseline name is needed.
            current_mean_ns = 0.0
            new_estimates = change_file.parent.parent / "new" / "estimates.json"
            if new_estimates.is_file():
                try:
                    with open(new_estimates, "r") as f:
                        new_data = json.load(f)
                    current_mean_ns = new_data.get("mean", {}).get("point_estimate", 0.0)
                except (json.JSONDecodeError, OSError):
                    current_mean_ns = 0.0
            absolute_delta_ns = 0.0
            have_abs = current_mean_ns > 0 and point_estimate != -1.0
            if have_abs:
                baseline_mean_ns = current_mean_ns / (1.0 + point_estimate)
                absolute_delta_ns = current_mean_ns - baseline_mean_ns

            # Determine change type
            if abs(change_percent) < self.threshold:
                change_type = ChangeType.NO_CHANGE
            elif have_abs and self.min_abs_ns > 0 and abs(absolute_delta_ns) < self.min_abs_ns:
                # Real ratio, but below the absolute noise floor.
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
                confidence=confidence,
                absolute_delta_ns=absolute_delta_ns,
            )
        except (json.JSONDecodeError, KeyError, ValueError, FileNotFoundError) as e:
            print(f"Warning: Could not parse {change_file}: {e}", file=sys.stderr)
            return None
        except Exception as e:
            print(f"Error processing {change_file}: {e}", file=sys.stderr)
            return None

    def generate_report(self, output_file: Path) -> int:
        """Generate JSON report of analysis. Returns exit code."""
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
                "absolute_delta_us": round(result.absolute_delta_ns / 1000.0, 2),
                "confidence": result.confidence
            }

            if result.change_type == ChangeType.REGRESSION:
                report["regressions"].append(entry)
            elif result.change_type == ChangeType.IMPROVEMENT:
                report["improvements"].append(entry)
            else:
                report["no_change"].append(entry)

        # Sort by absolute change
        report["regressions"].sort(key=lambda x: x["change_percent"], reverse=True)
        report["improvements"].sort(key=lambda x: x["change_percent"])

        # Write report
        output_file.parent.mkdir(parents=True, exist_ok=True)
        with open(output_file, 'w') as f:
            json.dump(report, f, indent=2)

        print(f"\nReport written to: {output_file}")

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
            for reg in report["regressions"]:
                print(f"  {reg['name']}")
                print(f"    Change: +{reg['change_percent']:.2f}% (slower)")
                print(f"    Effect: +{reg['absolute_delta_us']:.2f} us")
                print()

        if report["improvements"]:
            print(f"✅ IMPROVEMENTS: {len(report['improvements'])}")
            print("-" * 70)
            for imp in report["improvements"][:10]:  # Show top 10
                print(f"  {imp['name']}")
                print(f"    Change: {imp['change_percent']:.2f}% (faster)")
                print()
            if len(report["improvements"]) > 10:
                print(f"  ...and {len(report['improvements']) - 10} more improvements\n")

        if report["no_change"]:
            print(f"→ NO SIGNIFICANT CHANGE: {len(report['no_change'])}")

        print("="*70)


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description="Analyze Criterion benchmark results for regressions",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Analyze with default 20%% threshold
  %(prog)s --baseline target/criterion --current target/criterion --output report.json

  # Use 5%% threshold for strict checking
  %(prog)s --baseline baseline/ --current target/criterion --threshold 5 --output report.json
        """
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        required=True,
        help="Path to baseline benchmark results directory"
    )
    parser.add_argument(
        "--current",
        type=Path,
        required=True,
        help="Path to current benchmark results directory"
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=20.0,
        help="Regression threshold in percent (default: 20.0)"
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output file for JSON report"
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Show verbose output"
    )
    parser.add_argument(
        "--changed-since",
        type=float,
        default=None,
        metavar="EPOCH",
        help=(
            "Only analyse comparisons newer than this Unix timestamp. "
            "Criterion keeps old change/ directories, so a gate must ignore "
            "results left by previous runs."
        )
    )
    parser.add_argument(
        "--min-abs-ns",
        type=float,
        default=0.0,
        metavar="NS",
        help=(
            "Ignore changes smaller than this absolute amount. A percentage "
            "threshold alone lets microbenchmark noise block a gate."
        )
    )

    args = parser.parse_args()

    # Validate paths
    if not args.current.exists():
        print(f"Error: Current results directory not found: {args.current}",
              file=sys.stderr)
        return 1

    if args.verbose:
        print(f"Baseline: {args.baseline}")
        print(f"Current: {args.current}")
        print(f"Threshold: {args.threshold}%")
        print(f"Output: {args.output}")
        print()

    # Analyze
    analyzer = BenchmarkAnalyzer(
        args.baseline,
        args.current,
        args.threshold,
        args.changed_since,
        args.min_abs_ns,
    )
    results = analyzer.analyze()

    if args.verbose:
        print(f"Found {len(results)} benchmark results")

    # Generate report
    return analyzer.generate_report(args.output)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\nInterrupted", file=sys.stderr)
        sys.exit(130)
    except Exception as e:
        print(f"Fatal error: {e}", file=sys.stderr)
        sys.exit(1)
