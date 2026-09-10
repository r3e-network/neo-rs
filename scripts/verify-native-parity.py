#!/usr/bin/env python3
"""Verify Rust native-contract method parity against the C# reference implementation.

Why this exists
---------------
"Is the Neo N3 protocol fully implemented?" is not answerable by counting crates or
by asserting that tests pass. The only defensible answer is a *surface* comparison:
for every method the C# reference node exposes on a native contract, does the Rust
node expose the same method with the same name?

This script performs exactly that comparison, method by method, contract by contract,
by parsing both sources directly:

  * C#  (neo-project/neo @ master-n3): every ``[ContractMethod(...)]`` attribute.
    The contract-facing name is resolved with the *actual* rule from
    ``ContractMethodMetadata.cs``::

        Name = attribute.Name ?? member.Name;
        Name = Name.ToLowerInvariant()[0] + Name[1..];

    i.e. the attribute's ``Name`` wins, otherwise the C# method name, with the first
    character lower-cased. Guessing this rule instead of reading it would silently
    produce a wrong matrix, so it is replicated verbatim.

  * Rust (this repo): every ``safe "name"`` / ``unsafe "name"`` entry in the native
    contract method tables.

Caveats (deliberately narrow scope)
-----------------------------------
This compares *presence and naming* of native contract methods. It does NOT compare:
  * parameter types / arity / return types
  * hardfork activation windows (a method may exist on both sides but activate at
    different heights)
  * semantics, gas costs, or call flags
  * syscalls (interop services), RPC surface, or P2P messages

A green result therefore means "no missing or mis-named native method", not
"behaviourally identical". Those are different claims and must not be conflated.

Usage
-----
    python scripts/verify-native-parity.py                # compare, print report
    python scripts/verify-native-parity.py --json out.json
    python scripts/verify-native-parity.py --offline      # use cached C# sources only

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

CSHARP_BASE = "https://raw.githubusercontent.com/neo-project/neo"
# The audit targets Neo N3 v3.10.1, so that tag is the baseline. Do NOT default to
# master-n3: it is a moving development branch and has already diverged (e.g. it
# removed NeoToken.getCandidates, which v3.10.1 still exposes), which would report
# false "extra in Rust" diffs.
CSHARP_REF = os.environ.get("NEO_CSHARP_REF", "v3.10.1")

RUST_NATIVE_DIR = REPO_ROOT / "neo-core" / "src" / "smart_contract" / "native"

# contract key -> (C# source files, Rust method-table sources)
# Multiple C# files are needed where the class is `partial` (CryptoLib) or where the
# contract inherits contract methods from a base class (NEO/GAS <- FungibleToken).
# On the Rust side NEO/GAS likewise inherit their NEP-17 surface from
# FungibleToken::ft_nep17_methods(), so `fungible_token.rs` must be scanned too --
# the per-token metadata.rs only holds the token-specific methods.
CONTRACTS: dict[str, tuple[list[str], list[str]]] = {
    "ContractManagement": (["ContractManagement.cs"], ["contract_management/metadata.rs"]),
    "StdLib": (["StdLib.cs"], ["std_lib/metadata.rs"]),
    "CryptoLib": (["CryptoLib.cs", "CryptoLib.BLS12_381.cs"], ["crypto_lib/metadata.rs"]),
    "LedgerContract": (["LedgerContract.cs"], ["ledger_contract/metadata.rs"]),
    "NeoToken": (["FungibleToken.cs", "NeoToken.cs"], ["neo_token/metadata.rs", "fungible_token.rs"]),
    "GasToken": (["FungibleToken.cs", "GasToken.cs"], ["gas_token/metadata.rs", "fungible_token.rs"]),
    "PolicyContract": (["PolicyContract.cs"], ["policy_contract/metadata.rs"]),
    "RoleManagement": (["RoleManagement.cs"], ["role_management/metadata.rs"]),
    "OracleContract": (["OracleContract.cs"], ["oracle_contract/metadata.rs"]),
    "Notary": (["Notary.cs"], ["notary/metadata.rs"]),
    "Treasury": (["Treasury.cs"], ["treasury.rs"]),
}

# Matches the declaration that owns a [ContractMethod] attribute. Group 1 is the name.
# Both methods (`Type Name(`) and properties (`Type Name {`) occur in C# native
# contracts -- `Symbol` and `Decimals` are properties -- so both must be recognised,
# and whichever appears FIRST after the attribute is the owner. Matching only methods
# makes the parser skip a property and latch onto a later unrelated method.
CSHARP_DECL_RE = re.compile(
    r"\b(?:public|private|protected|internal)\s+"
    r"(?:static\s+|override\s+|virtual\s+|abstract\s+|async\s+|new\s+|sealed\s+|extern\s+|partial\s+)*"
    # The type class must admit parentheses for tuple return types such as
    # `internal (ECPoint PublicKey, BigInteger Votes)[] GetCandidates(...)`, otherwise
    # those methods are silently skipped and reported as spurious diffs.
    r"[\w<>\[\](),.\?\s]+?\b(\w+)\s*(?=\(|\{)",
)


def fetch_text(url: str, offline: bool = False) -> str:
    """Fetch `url`, caching under .cache/csharp-parity."""
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    cache_file = CACHE_DIR / (url.replace("://", "_").replace("/", "_"))
    if cache_file.exists():
        return cache_file.read_text(encoding="utf-8")
    if offline:
        raise FileNotFoundError(f"offline mode and no cache for {url}")
    try:
        with urllib.request.urlopen(url, timeout=60) as resp:
            text = resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"failed to fetch {url}: HTTP {exc.code}") from exc
    cache_file.write_text(text, encoding="utf-8")
    return text


def _attribute_span(src: str, start: int) -> tuple[int, int]:
    """Return (start, end) of the `[ContractMethod...]` attribute beginning at `start`.

    Scans manually so that nested parentheses inside the attribute do not truncate it.
    """
    depth = 0
    i = start
    while i < len(src):
        ch = src[i]
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
            if depth == 0:
                return start, i + 1
        i += 1
    return start, len(src)


def extract_csharp_methods(src: str) -> set[str]:
    """Extract contract-facing method names from C# source.

    Replicates ContractMethodMetadata's naming rule verbatim.
    """
    names: set[str] = set()
    for match in re.finditer(r"\[ContractMethod", src):
        _, end = _attribute_span(src, match.start())
        attribute = src[match.start() : end]

        name_match = re.search(r'Name\s*=\s*"([^"]+)"', attribute)
        if name_match:
            raw = name_match.group(1)
        else:
            # The attribute's owning declaration is the next one after it. Bound the
            # search at the next attribute so a malformed match cannot run away.
            tail = src[end:]
            next_attr = tail.find("[ContractMethod")
            if next_attr != -1:
                tail = tail[:next_attr]
            decl = CSHARP_DECL_RE.search(tail)
            if decl is None:
                continue
            raw = decl.group(1)

        # ContractMethodMetadata: Name.ToLowerInvariant()[0] + Name[1..]
        names.add(raw[0].lower() + raw[1:] if raw else raw)
    return names


def extract_rust_methods(paths: list[str]) -> set[str]:
    """Extract method names from the Rust native-contract method tables."""
    names: set[str] = set()
    for rel in paths:
        path = RUST_NATIVE_DIR / rel
        if not path.exists():
            raise FileNotFoundError(f"missing Rust source: {path}")
        src = path.read_text(encoding="utf-8")
        names.update(re.findall(r'\b(?:safe|unsafe)\s+"([A-Za-z_][A-Za-z0-9_]*)"', src))
    return names


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", help="write the machine-readable report here")
    parser.add_argument("--offline", action="store_true", help="use cached C# sources only")
    parser.add_argument("--csharp-ref", default=CSHARP_REF, help="C# git ref to compare against")
    args = parser.parse_args()

    report: dict[str, dict] = {}
    total_missing: list[str] = []
    total_extra: list[str] = []

    for contract, (cs_files, rs_files) in CONTRACTS.items():
        csharp: set[str] = set()
        for name in cs_files:
            url = f"{CSHARP_BASE}/{args.csharp_ref}/src/Neo/SmartContract/Native/{name}"
            csharp |= extract_csharp_methods(fetch_text(url, offline=args.offline))

        rust = extract_rust_methods(rs_files)

        missing = sorted(csharp - rust)  # in C#, absent in Rust
        extra = sorted(rust - csharp)  # in Rust, absent in C#
        report[contract] = {
            "csharp_methods": len(csharp),
            "rust_methods": len(rust),
            "matched": len(csharp & rust),
            "missing_in_rust": missing,
            "extra_in_rust": extra,
            "parity": not missing and not extra,
        }
        total_missing += [f"{contract}.{m}" for m in missing]
        total_extra += [f"{contract}.{m}" for m in extra]

    width = max(len(c) for c in CONTRACTS)
    print(f"Native contract method parity (C# {args.csharp_ref} vs Rust)")
    print("=" * 72)
    print(f"{'contract'.ljust(width)}  {'C#':>4} {'Rust':>5} {'match':>6}  status")
    print("-" * 72)
    for contract, row in report.items():
        status = "OK" if row["parity"] else "DIFF"
        print(
            f"{contract.ljust(width)}  {row['csharp_methods']:>4} {row['rust_methods']:>5} "
            f"{row['matched']:>6}  {status}"
        )
        for method in row["missing_in_rust"]:
            print(f"    MISSING in Rust : {method}")
        for method in row["extra_in_rust"]:
            print(f"    EXTRA in Rust   : {method}")
    print("-" * 72)
    ok = not total_missing and not total_extra
    print(f"missing: {len(total_missing)}   extra: {len(total_extra)}   -> {'PARITY' if ok else 'DIFFERENCES'}")

    if args.json:
        # Create the parent directory: CI passes a path under reports/, which need not
        # exist. Without this the run fails with exit code 2 *after* successfully
        # computing parity, which reads as a parity failure when it is only an IO slip.
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"\nJSON report: {args.json}")

    return 0 if ok else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (FileNotFoundError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(2)
