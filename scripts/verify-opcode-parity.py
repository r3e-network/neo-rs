#!/usr/bin/env python3
"""Verify Rust NeoVM opcode parity against the C# v3.10.1 reference.

Third of the surface-parity checks (see also verify-native-parity.py and
verify-syscall-parity.py). Covers the instruction set: every opcode in the C#
reference must exist in the Rust VM under the same name *and* with the same byte
value.

Comparing values, not just names, is the point: a name can match while its byte
disagrees, and a wrong opcode byte corrupts every script that uses it rather than
failing loudly.

Source note
-----------
The VM is NOT in the main neo repo at v3.10.1 -- it was extracted into its own
repository. C# opcodes therefore come from `neo-project/neo-vm` at tag `v3.10.1`
(`src/Neo.VM/OpCode.cs`), while everything else in this repo's parity checks comes
from `neo-project/neo` at `v3.10.1`. Both refs are pinned; `master` is a moving
branch and is never used as a baseline.

Parsing caveat
--------------
The C# enum's final member (`ASSERTMSG = 0xE1`) has no trailing comma, unlike every
other member. A regex that requires the comma silently drops it and reports a phantom
"extra in Rust" opcode. The trailing comma must be optional.

Usage
-----
    python scripts/verify-opcode-parity.py
    python scripts/verify-opcode-parity.py --json reports/opcode-parity.json

Exit codes: 0 = full parity, 1 = differences found, 2 = tooling/IO error.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CACHE_DIR = REPO_ROOT / ".cache" / "csharp-parity"

# The VM lives in its own repo (see module docstring).
CSHARP_BASE = "https://raw.githubusercontent.com/neo-project/neo-vm"
CSHARP_REF = os.environ.get("NEO_CSHARP_VM_REF", "v3.10.1")
CSHARP_PATH = "src/Neo.VM/OpCode.cs"

RUST_OPCODE_FILE = REPO_ROOT / "neo-vm" / "src" / "vm" / "opcode.rs"

# C# enum members are 4-space indented; the trailing comma is optional because the
# final member (ASSERTMSG) has none.
CSHARP_OPCODE_RE = re.compile(r"^\s{4}([A-Z][A-Z0-9_]*)\s*=\s*(0x[0-9A-Fa-f]+|\d+)\s*,?\s*$", re.M)
# Rust: `NAME = 0xNN, operand_size = ...`
RUST_OPCODE_RE = re.compile(r"^\s*([A-Z][A-Z0-9_]*)\s*=\s*(0x[0-9A-Fa-f]+|\d+)\s*,", re.M)


def fetch_text(url: str) -> str:
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    cache_file = CACHE_DIR / (url.replace("://", "_").replace("/", "_"))
    if cache_file.exists():
        return cache_file.read_text(encoding="utf-8")
    try:
        with urllib.request.urlopen(url, timeout=60) as resp:
            text = resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"failed to fetch {url}: HTTP {exc.code}") from exc
    cache_file.write_text(text, encoding="utf-8")
    return text


def parse(src: str, pattern: re.Pattern[str]) -> dict[str, int]:
    return {name: int(value, 0) for name, value in pattern.findall(src)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", help="write the machine-readable report here")
    parser.add_argument("--csharp-ref", default=CSHARP_REF, help="C# neo-vm git ref to compare against")
    args = parser.parse_args()

    csharp = parse(fetch_text(f"{CSHARP_BASE}/{args.csharp_ref}/{CSHARP_PATH}"), CSHARP_OPCODE_RE)
    rust = parse(RUST_OPCODE_FILE.read_text(encoding="utf-8"), RUST_OPCODE_RE)

    missing = sorted(set(csharp) - set(rust))
    extra = sorted(set(rust) - set(csharp))
    mismatched = sorted(
        (name, f"0x{rust[name]:02x}", f"0x{csharp[name]:02x}")
        for name in set(csharp) & set(rust)
        if csharp[name] != rust[name]
    )

    print(f"NeoVM opcode parity (C# neo-vm {args.csharp_ref} vs Rust)")
    print("=" * 60)
    print(f"C# opcodes  : {len(csharp)}")
    print(f"Rust opcodes: {len(rust)}")
    print(f"matched     : {len(set(csharp) & set(rust))}")
    for name in missing:
        print(f"  MISSING in Rust : {name}")
    for name in extra:
        print(f"  EXTRA in Rust   : {name}")
    for name, rust_val, cs_val in mismatched:
        print(f"  VALUE MISMATCH  : {name} rust={rust_val} csharp={cs_val}")
    print("-" * 60)

    ok = not missing and not extra and not mismatched
    print(f"-> {'PARITY' if ok else 'DIFFERENCES'}")

    if args.json:
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(
                {
                    "csharp_opcodes": len(csharp),
                    "rust_opcodes": len(rust),
                    "matched": len(set(csharp) & set(rust)),
                    "missing_in_rust": missing,
                    "extra_in_rust": extra,
                    "value_mismatches": [
                        {"opcode": n, "rust": r, "csharp": c} for n, r, c in mismatched
                    ],
                    "parity": ok,
                },
                indent=2,
            ),
            encoding="utf-8",
        )
        print(f"JSON report: {args.json}")

    return 0 if ok else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (FileNotFoundError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(2)
