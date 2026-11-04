#!/usr/bin/env python3
"""Summarize NEO_REPLAY_PROFILE phase timings."""

from collections import defaultdict
from pathlib import Path
import re
import statistics
import sys

scale = {"ns": 1e-6, "µs": 1e-3, "ms": 1.0, "s": 1e3}
pattern = re.compile(r"profile block=(\d+) ([a-z_]+)=([0-9.]+)(ns|µs|ms|s)\b")
groups = defaultdict(list)
for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    match = pattern.search(line)
    if match:
        groups[match.group(2)].append(float(match.group(3)) * scale[match.group(4)])
for name, values in sorted(groups.items()):
    ordered = sorted(values)
    p95 = ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]
    print(
        f"{name}: count={len(values)} total_ms={sum(values):.3f} "
        f"mean_ms={statistics.mean(values):.3f} p50_ms={statistics.median(values):.3f} "
        f"p95_ms={p95:.3f} max_ms={max(values):.3f}"
    )
