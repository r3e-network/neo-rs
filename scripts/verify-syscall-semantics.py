#!/usr/bin/env python3
"""Verify syscall *semantics* (not just names) against the C# v3.10.1 reference.

`verify-syscall-parity.py` proves the Rust node registers the same syscall *names*
and derives the same *hashes* as C#. It does not prove the two implementations agree
on anything you can observe while executing a contract. A syscall can be registered
under the correct name and hash and still be observably different:

  * charged a different fixed price (gas / fee divergence),
  * guarded by a different required CallFlags (Authorization fault where C# succeeds),
  * only available from a different hardfork onwards (activation-height divergence).

This script closes those three gaps by extracting, for every one of the 43 registered
syscalls on both sides, the triple

    (fixed price, required call flags, activation hardfork)

and diffing the triples. That triple fully determines the *observable admission and
costing semantics* of a syscall: what the dispatcher checks before calling the handler,
and what it charges for the call. (Handler return-value semantics are a separate, deeper
layer -- see the note in the report footer.)

Extraction notes
----------------
C#  `Register("System.X", nameof(Handler), <price>, <flags>[, Hardfork.HF_Y])`
    - <price> is a `long`; `1 << 15` and `0` are literal, `CheckSigPrice` is a const
      resolved from the same file.
    - <flags> is a `CallFlags` bitwise-or of enum members, or `CallFlags.None`.
Rust `register_host_service("System.X", <price>, CallFlags::..., handler)`
    - a registration wrapped in `if engine.is_hardfork_enabled(Hardfork::HfY) { ... }`
      is hardfork-gated; its activation is HfY. Everything else is genesis (HF_None).
    - Note: unlike C#, Rust *conditionally registers* rather than keeping one descriptor
      with a hardfork field. Both are equivalent for observability -- the descriptor is
      absent before the fork either way -- so we normalise C# `Hardfork?` and Rust
      `if is_hardfork_enabled` to the same "activation" attribute.

Flag normalisation
------------------
C# and Rust name the flags differently (`CallFlags.ReadStates` vs `CallFlags::READ_STATES`)
and express combinations differently (`A | B` vs `A | B`). We lower both to a set of
canonical lowercase tokens (`readstates`, `writestates`, `allowcall`, `allownotify`,
`allowmodify`, `states`, `all`) and compare the sets, so `CallFlags.States` ==
`ReadStates | WriteStates` only if the C# source actually spells it that way.

Usage
-----
    python scripts/verify-syscall-semantics.py
    python scripts/verify-syscall-semantics.py --json reports/syscall-semantics.json

Exit codes: 0 = full semantic parity, 1 = differences found, 2 = tooling/IO error.
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
CSHARP_REF = os.environ.get("NEO_CSHARP_REF", "v3.10.1")
CSHARP_DIR = "src/Neo/SmartContract"

CSHARP_FILES = [
    "ApplicationEngine.cs",
    "ApplicationEngine.Contract.cs",
    "ApplicationEngine.Crypto.cs",
    "ApplicationEngine.Storage.cs",
    "ApplicationEngine.Runtime.cs",
    "ApplicationEngine.Iterator.cs",
]

RUST_REGISTER_DIR = REPO_ROOT / "neo-core" / "src" / "smart_contract" / "application_engine"

# ---------------------------------------------------------------------------
# C# Hardfork enum -> canonical string. Order is the C# activation order.
# ---------------------------------------------------------------------------
CSHARP_HARDFORKS = [
    "HF_Aspidochelone",
    "HF_Basilisk",
    "HF_Cockatrice",
    "HF_Domovoi",
    "HF_Echidna",
    "HF_Faun",
    "HF_Gorgon",
    "HF_Huyao",
]

# Rust enum variant -> canonical "HF_*" string (strip the `Hf` prefix, re-add `HF_`).
RUST_HARDFORK_RE = re.compile(r"Hardfork::(Hf[A-Za-z0-9]+)")


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


# ---------------------------------------------------------------------------
# Call-flag normalisation
# ---------------------------------------------------------------------------

def normalise_flags(expr: str) -> frozenset[str]:
    """Lower a flag expression to a canonical set of tokens.

    Handles C# `CallFlags.A | CallFlags.B` and Rust `CallFlags::A | CallFlags::B`.
    `None` / `NONE` -> empty set.
    """
    expr = expr.strip()
    if not expr:
        return frozenset()
    tokens = re.split(r"[|+]", expr)
    out: set[str] = set()
    for tok in tokens:
        tok = tok.strip()
        tok = tok.replace("CallFlags::", "").replace("CallFlags.", "")
        tok = tok.lower().replace("_", "")
        if tok in ("none", ""):
            continue
        out.add(tok)
    return frozenset(out)


# ---------------------------------------------------------------------------
# C# extraction
# ---------------------------------------------------------------------------

# Register("System.X", nameof(H) | "H", <price>, <flags>[ , Hardfork.HF_Y])
CSHARP_REGISTER_RE = re.compile(
    r'Register\(\s*"([^"]+)"\s*,\s*(?:nameof\(\s*(\w+)\s*\)|"(\w+)")'
    r'\s*,\s*([^,]+?)\s*,\s*(.+?)\s*(?:,\s*Hardfork\.(\w+)\s*)?\)\s*;',
    re.S,
)


def extract_csharp_semantics(ref: str) -> dict[str, dict]:
    out: dict[str, dict] = {}
    consts: dict[str, str] = {}

    sources = []
    for name in CSHARP_FILES:
        url = f"{CSHARP_BASE}/{ref}/{CSHARP_DIR}/{name}"
        try:
            sources.append((name, fetch_text(url)))
        except RuntimeError as exc:
            # A partial class may not exist for this ref; that is not fatal as long
            # as the ones that do exist carry every registration.
            print(f"warning: {exc}", file=sys.stderr)

    # Collect simple `const long X = <expr>;` definitions so symbolic prices resolve.
    for _, src in sources:
        for m in re.finditer(r"const\s+long\s+(\w+)\s*=\s*([^;]+);", src):
            consts[m.group(1)] = m.group(2).strip()

    def resolve_price(raw: str) -> str:
        raw = raw.strip()
        if raw in consts:
            return consts[raw]
        return raw

    for srcname, src in sources:
        for m in CSHARP_REGISTER_RE.finditer(src):
            name = m.group(1)
            price = resolve_price(m.group(4))
            flags = normalise_flags(m.group(5))
            # The C# source spells hardforks as `Hardfork.HF_Faun`; capture group 6
            # is `HF_Faun` (already prefixed), so do NOT prepend again.
            hardfork = m.group(6) if m.group(6) else None
            out[name] = {
                "price_expr": price,
                "price": eval(i64_expr(price)) if i64_expr(price) else None,
                "flags": sorted(flags),
                "hardfork": hardfork,
                "source": srcname,
            }
    return out


def i64_expr(expr: str) -> str | None:
    """Validate a C# long expression is safe to evaluate, return it if so."""
    if not re.fullmatch(r"[0-9\s<()|+*L\-]+", expr):
        return None
    return expr.replace("L", "")


# ---------------------------------------------------------------------------
# Rust extraction
# ---------------------------------------------------------------------------

# register_host_service( "System.X", <price>, CallFlags::..., handler, )
RUST_REGISTER_RE = re.compile(
    r'register_host_service\(\s*"([^"]+)"\s*,\s*([^,]+?)\s*,\s*(.+?)\s*,\s*\w+\s*,?\s*\)',
    re.S,
)

# `if ... is_hardfork_enabled(Hardfork::HfX ...) {` ... matching `}`
RUST_GATE_RE = re.compile(
    r"is_hardfork_enabled\(\s*Hardfork::(Hf[A-Za-z0-9]+)",
)


def rust_hardfork_canonical(variant: str) -> str:
    """HfFaun -> HF_Faun."""
    return "HF_" + variant[2:]


def debug_long_expr(expr: str) -> str | None:
    """Validate a Rust i64 expression, return it if safe to evaluate in Python.

    Identifier resolution happens BEFORE underscore stripping, otherwise
    `CHECK_SIG_PRICE` would be mangled to `CHECKSIGPRICE` and never match a const.
    """
    if not re.fullmatch(r"[0-9\s<()|+*_A-Za-z:\-]+", expr):
        return None
    resolved = expr
    for ident in sorted(set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", resolved)), key=len, reverse=True):
        if ident in RUST_CONSTS:
            resolved = re.sub(rf"\b{re.escape(ident)}\b", f"({RUST_CONSTS[ident]})", resolved)
        else:
            return None  # unresolvable symbol
    resolved = resolved.replace("_", "")
    if not re.fullmatch(r"[0-9\s<()|+*\-]+", resolved):
        return None
    return resolved


RUST_CONSTS: dict[str, str] = {}


def extract_rust_semantics() -> dict[str, dict]:
    global RUST_CONSTS
    RUST_CONSTS = {}

    files = sorted(RUST_REGISTER_DIR.glob("*.rs"))
    texts: dict[Path, str] = {p: p.read_text(encoding="utf-8") for p in files}

    # i64 consts anywhere in the registration directory (e.g. CHECK_SIG_PRICE).
    # Match both `const X: i64 = ...` and `pub const X: i64 = ...`; the leading
    # `pub` is optional on purpose (SYSTEM_CONTRACT_CALL_PRICE is private).
    for src in texts.values():
        for m in re.finditer(
            r"(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*i64\s*=\s*([^;]+);", src
        ):
            RUST_CONSTS.setdefault(m.group(1), m.group(2).strip())

    out: dict[str, dict] = {}
    for path, src in texts.items():
        gate_spans = find_gate_spans(src)
        for m in RUST_REGISTER_RE.finditer(src):
            name = m.group(1)
            price_raw = m.group(2).strip()
            flags = normalise_flags(m.group(3))
            start = m.start()
            gated_by = None
            for (gs, ge, hf) in gate_spans:
                if gs <= start < ge:
                    gated_by = rust_hardfork_canonical(hf)
                    break
            price_expr = debug_long_expr(price_raw)
            out[name] = {
                "price_expr": price_raw,
                "price": eval(price_expr) if price_expr else None,
                "flags": sorted(flags),
                "hardfork": gated_by,
                "source": path.name,
            }
    return out


def find_gate_spans(src: str) -> list[tuple[int, int, str]]:
    """Return (block_start, block_end, hardfork_variant) for each hardfork gate.

    Uses brace matching so nested registrations are attributed correctly.
    """
    spans: list[tuple[int, int, str]] = []
    for m in RUST_GATE_RE.finditer(src):
        variant = m.group(1)
        # Find the opening brace of the guarded block.
        brace = src.find("{", m.end())
        if brace == -1:
            continue
        depth = 0
        i = brace
        while i < len(src):
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        spans.append((brace, i, variant))
    return spans


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", help="write the machine-readable report here")
    parser.add_argument("--csharp-ref", default=CSHARP_REF)
    args = parser.parse_args()

    csharp = extract_csharp_semantics(args.csharp_ref)
    rust = extract_rust_semantics()

    common = sorted(set(csharp) & set(rust))
    only_cs = sorted(set(csharp) - set(rust))
    only_rs = sorted(set(rust) - set(csharp))

    diffs: list[dict] = []
    for name in common:
        c, r = csharp[name], rust[name]
        row: dict = {"name": name, "fields": []}
        if c["price"] != r["price"]:
            row["fields"].append(
                {
                    "field": "price",
                    "csharp": f"{c['price_expr']} = {c['price']}",
                    "rust": f"{r['price_expr']} = {r['price']}",
                }
            )
        if c["flags"] != r["flags"]:
            row["fields"].append(
                {"field": "flags", "csharp": c["flags"], "rust": r["flags"]}
            )
        if c["hardfork"] != r["hardfork"]:
            row["fields"].append(
                {
                    "field": "hardfork",
                    "csharp": c["hardfork"],
                    "rust": r["hardfork"],
                }
            )
        if row["fields"]:
            diffs.append(row)

    print(f"Syscall semantic parity (C# {args.csharp_ref} vs Rust)")
    print("=" * 74)
    print(f"C#  registered syscalls : {len(csharp)}")
    print(f"Rust registered syscalls: {len(rust)}")
    print(f"comparable              : {len(common)}")
    for n in only_cs:
        print(f"  MISSING in Rust : {n}")
    for n in only_rs:
        print(f"  EXTRA in Rust   : {n}")
    print("-" * 74)

    if not diffs:
        print("price / call-flags / activation-hardfork: ALL MATCH")
    else:
        for row in diffs:
            print(f"DIFF {row['name']}")
            for f in row["fields"]:
                print(f"   {f['field']:9s} csharp={f['csharp']!r}")
                print(f"   {'':9s} rust  ={f['rust']!r}")

    # Independent integrity check: every C# price must have resolved to an integer.
    unresolved_cs = [n for n, v in csharp.items() if v["price"] is None]
    unresolved_rs = [n for n, v in rust.items() if v["price"] is None]
    if unresolved_cs:
        print("-" * 74)
        print(f"UNRESOLVED C# prices ({len(unresolved_cs)}): {unresolved_cs}")
    if unresolved_rs:
        print("-" * 74)
        print(f"UNRESOLVED Rust prices ({len(unresolved_rs)}): {unresolved_rs}")

    print("-" * 74)
    ok = not diffs and not only_cs and not only_rs and not unresolved_cs and not unresolved_rs
    print(f"-> {'SEMANTIC PARITY' if ok else 'DIFFERENCES'}")

    if args.json:
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(
                {
                    "csharp_ref": args.csharp_ref,
                    "csharp_only": only_cs,
                    "rust_only": only_rs,
                    "compared": len(common),
                    "differences": diffs,
                    "unresolved_csharp_prices": unresolved_cs,
                    "unresolved_rust_prices": unresolved_rs,
                    "parity": ok,
                    "csharp_table": csharp,
                    "rust_table": rust,
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
