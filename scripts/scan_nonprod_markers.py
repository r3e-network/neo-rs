#!/usr/bin/env python3
"""
Scan the repo for "non-production" markers (TODO/FIXME/placeholder/etc).

This is intended to help systematically locate unfinished logic that could
impact protocol correctness, stability, or security.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, List, Optional


REPO_ROOT = Path(__file__).resolve().parents[1]


DEFAULT_EXCLUDES = [
    ".git",
    "target",
    "neo_csharp",  # submodule/reference; noise for Rust node correctness work
]


PATTERNS = [
    r"\bTODO\b",
    r"\bFIXME\b",
    r"\bHACK\b",
    r"\bXXX\b",
    r"\bWIP\b",
    r"\bTEMP(?:ORARILY)?\b",
    r"\bFOR NOW\b",
    r"\bPLACEHOLDER\b",
    r"\bSIMPLIFIED\b",
    r"\bNOT PRODUCTION\b",
    r"\bNON[- ]PRODUCTION\b",
    r"\bIN REAL IMPLEMENTATION\b",
    r"\bIN PRODUCTION\b",
    r"\bSTUB\b",
    r"\bUNIMPLEMENTED\b",
    r"\bNOT IMPLEMENTED\b",
]


@dataclass(frozen=True)
class Hit:
    path: str
    line: int
    column: int
    pattern: str
    text: str


def _rg_available() -> bool:
    try:
        subprocess.run(["rg", "--version"], check=True, stdout=subprocess.DEVNULL)
        return True
    except Exception:
        return False


def _run_rg(pattern: str, excludes: List[str], include_glob: Optional[str]) -> List[Hit]:
    cmd = [
        "rg",
        "--no-heading",
        "--line-number",
        "--column",
        "--pcre2",
        "--ignore-case",
        pattern,
        ".",
    ]

    if include_glob:
        cmd.extend(["-g", include_glob])

    for ex in excludes:
        cmd.extend(["-g", f"!{ex}/**"])

    proc = subprocess.run(
        cmd,
        cwd=str(REPO_ROOT),
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if proc.returncode not in (0, 1):
        raise RuntimeError(f"rg failed for pattern {pattern!r}: {proc.stderr.strip()}")

    hits: List[Hit] = []
    for line in proc.stdout.splitlines():
        # Format: path:line:col:text
        parts = line.split(":", 3)
        if len(parts) != 4:
            continue
        path, ln, col, text = parts
        try:
            hits.append(
                Hit(
                    path=path,
                    line=int(ln),
                    column=int(col),
                    pattern=pattern,
                    text=text,
                )
            )
        except ValueError:
            continue
    return hits


def _iter_hits(excludes: List[str], include_glob: Optional[str]) -> Iterable[Hit]:
    for pattern in PATTERNS:
        yield from _run_rg(pattern, excludes, include_glob)


def main() -> int:
    parser = argparse.ArgumentParser(description="Scan for non-production markers")
    parser.add_argument(
        "--include",
        dest="include_glob",
        default=None,
        help="Optional ripgrep glob include filter (e.g. '*.rs' or 'neo-*/**/*.rs')",
    )
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="Path prefix to exclude (repeatable). Default excludes include .git/ target/ neo_csharp/",
    )
    parser.add_argument(
        "--format",
        choices=("text", "json"),
        default="text",
        help="Output format",
    )
    parser.add_argument(
        "--max",
        type=int,
        default=0,
        help="If >0, exit non-zero when hits exceed this threshold",
    )
    args = parser.parse_args()

    if not _rg_available():
        print("error: ripgrep (rg) is required", file=sys.stderr)
        return 2

    excludes = DEFAULT_EXCLUDES + list(args.exclude)
    hits = sorted(_iter_hits(excludes, args.include_glob), key=lambda h: (h.path, h.line, h.column))

    if args.format == "json":
        try:
            print(json.dumps([h.__dict__ for h in hits], indent=2))
        except BrokenPipeError:
            return 0
    else:
        try:
            for h in hits:
                print(
                    f"{h.path}:{h.line}:{h.column}: {h.text.strip()}  [pattern={h.pattern}]"
                )
        except BrokenPipeError:
            return 0
        print(f"\nTotal hits: {len(hits)}", file=sys.stderr)

    if args.max > 0 and len(hits) > args.max:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
