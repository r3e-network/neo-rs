#!/usr/bin/env python3
"""Verify Rust interop-service (syscall) parity against the C# v3.10.1 reference.

Companion to `verify-native-parity.py`, which covers native contract methods. This
script covers the other half of the execution surface: the `System.*` syscalls a
contract can invoke via the SYSCALL opcode.

Two independent checks:

1. **Registration parity.** C# registers syscalls with
   `ApplicationEngine.Register("System.X", ...)` (spread across the ApplicationEngine
   partial classes). Rust registers them with `register_host_service("System.X", ...)`.
   The two name sets must be identical.

   Getting the Rust surface right matters: `neo-vm/src/host/syscall.rs` also contains a
   table of `"System.X" => 0xhash` entries, but that table only backs
   `syscall_arg_count()` and a test helper -- it is NOT the dispatch surface and it
   contains entries (System.Contract.Create / Update) that are not registered on either
   side. Diffing against it instead of against `register_host_service` reports two
   phantom "extra in Rust" syscalls.

2. **Hash parity.** C# derives a syscall's identifier as
   `BitConverter.ToUInt32(SHA256(ASCII(name)), 0)` -- a little-endian uint32 from the
   first four bytes. Every hash in the Rust table is recomputed from its name with that
   rule. Names can match while hashes disagree, and a hash mismatch makes every SYSCALL
   fault at runtime, so this is checked explicitly rather than assumed.

Usage
-----
    python scripts/verify-syscall-parity.py
    python scripts/verify-syscall-parity.py --json reports/syscall-parity.json

Exit codes: 0 = full parity, 1 = differences found, 2 = tooling/IO error.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import struct
import sys
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CACHE_DIR = REPO_ROOT / ".cache" / "csharp-parity"

CSHARP_BASE = "https://raw.githubusercontent.com/neo-project/neo"
CSHARP_REF = os.environ.get("NEO_CSHARP_REF", "v3.10.1")
CSHARP_DIR = "src/Neo/SmartContract"

# Register() is spread over the ApplicationEngine partial classes; Helper and
# OpCodePrices register nothing but are included so a future move is not missed.
CSHARP_FILES = [
    "ApplicationEngine.cs",
    "ApplicationEngine.Contract.cs",
    "ApplicationEngine.Crypto.cs",
    "ApplicationEngine.Helper.cs",
    "ApplicationEngine.Iterator.cs",
    "ApplicationEngine.OpCodePrices.cs",
    "ApplicationEngine.Runtime.cs",
    "ApplicationEngine.Storage.cs",
]

# Where Rust registers dispatchable syscalls.
RUST_REGISTER_DIR = REPO_ROOT / "neo-core" / "src" / "smart_contract" / "application_engine"
# Name -> hash table used for arg counts (also carries the hashes we verify).
RUST_HASH_TABLE = REPO_ROOT / "neo-vm" / "src" / "host" / "syscall.rs"


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


def csharp_syscall_hash(name: str) -> int:
    """C# InteropDescriptor hash: BitConverter.ToUInt32(SHA256(ASCII(name)), 0)."""
    return struct.unpack("<I", hashlib.sha256(name.encode("ascii")).digest()[:4])[0]


def extract_csharp_syscalls(ref: str) -> set[str]:
    names: set[str] = set()
    for name in CSHARP_FILES:
        url = f"{CSHARP_BASE}/{ref}/{CSHARP_DIR}/{name}"
        names |= set(re.findall(r'Register\(\s*"([^"]+)"', fetch_text(url)))
    return names


def extract_rust_syscalls() -> set[str]:
    """Names passed to register_host_service (the real dispatch surface)."""
    names: set[str] = set()
    for path in sorted(RUST_REGISTER_DIR.glob("*.rs")):
        names |= set(re.findall(r'register_host_service\(\s*"([^"]+)"', path.read_text(encoding="utf-8")))
    return names


def verify_hashes() -> tuple[int, int, list[str]]:
    """Recompute every Rust syscall hash with the C# rule. Returns (ok, total, mismatches)."""
    src = RUST_HASH_TABLE.read_text(encoding="utf-8")
    pairs = re.findall(r'"(System\.[A-Za-z0-9_.]+)"\s*=>\s*0x([0-9a-fA-F_]+)', src)
    mismatches: list[str] = []
    for name, raw in pairs:
        expected = csharp_syscall_hash(name)
        actual = int(raw.replace("_", ""), 16)
        if expected != actual:
            mismatches.append(f"{name}: rust=0x{actual:08x} csharp_rule=0x{expected:08x}")
    return len(pairs) - len(mismatches), len(pairs), mismatches


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", help="write the machine-readable report here")
    parser.add_argument("--csharp-ref", default=CSHARP_REF, help="C# git ref to compare against")
    args = parser.parse_args()

    csharp = extract_csharp_syscalls(args.csharp_ref)
    rust = extract_rust_syscalls()
    missing = sorted(csharp - rust)
    extra = sorted(rust - csharp)

    hash_ok, hash_total, hash_bad = verify_hashes()

    print(f"Interop service (syscall) parity (C# {args.csharp_ref} vs Rust)")
    print("=" * 60)
    print(f"C# registered syscalls : {len(csharp)}")
    print(f"Rust registered syscalls: {len(rust)}")
    print(f"matched                 : {len(csharp & rust)}")
    for name in missing:
        print(f"  MISSING in Rust : {name}")
    for name in extra:
        print(f"  EXTRA in Rust   : {name}")
    print("-" * 60)
    print(f"syscall hash derivation : {hash_ok}/{hash_total} match C# rule")
    for line in hash_bad:
        print(f"  HASH MISMATCH   : {line}")
    print("-" * 60)

    ok = not missing and not extra and not hash_bad
    print(f"-> {'PARITY' if ok else 'DIFFERENCES'}")

    if args.json:
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(
                {
                    "csharp_syscalls": sorted(csharp),
                    "rust_syscalls": sorted(rust),
                    "missing_in_rust": missing,
                    "extra_in_rust": extra,
                    "hash_checked": hash_total,
                    "hash_matched": hash_ok,
                    "hash_mismatches": hash_bad,
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
