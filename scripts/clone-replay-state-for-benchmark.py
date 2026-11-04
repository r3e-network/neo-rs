#!/usr/bin/env python3
"""Clone immutable RocksDB SSTs by hard link for an isolated replay benchmark."""

from pathlib import Path
import shutil
import sys

source = Path(sys.argv[1]).resolve()
destination = Path(sys.argv[2]).resolve()
if not source.is_dir():
    raise ValueError(f"missing source: {source}")
if destination.exists():
    raise ValueError(f"destination already exists: {destination}")
destination.mkdir(parents=True)
linked = copied = 0
for entry in source.iterdir():
    if not entry.is_file() or entry.name == "LOCK" or entry.name.startswith("LOG"):
        continue
    target = destination / entry.name
    if entry.suffix.lower() in {".sst", ".ldb"}:
        target.hardlink_to(entry)
        linked += 1
    else:
        shutil.copy2(entry, target)
        copied += 1
(destination / "BENCHMARK_ONLY").write_text(
    "Isolated benchmark output; production checkpoint guards do not apply.\n",
    encoding="utf-8",
)
print(f"cloned {source} to {destination}: linked={linked} copied={copied}")
