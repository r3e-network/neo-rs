#!/usr/bin/env python3
"""Verify native-contract method *semantics* against the C# v3.10.1 reference.

`verify-native-parity.py` answers "does Rust expose the same method names as C#?".
It deliberately does not look at anything else (its docstring says so). A method can
have the right name and still be observably wrong:

  * different parameter types   -> the manifest ABI differs, so calls that C# accepts
                                  fault with "the type of the argument does not match"
                                  (and vice versa),
  * different return type       -> the manifest ABI differs,
  * different CpuFee            -> the same call costs different gas/fees,
  * different RequiredCallFlags -> a call C# allows faults on `Authorization`,
  * different Active/Deprecated -> the method is absent/present over a different range
                                  of block heights (activation divergence).

This script extracts all of that on both sides and diffs it per method.

Reference semantics (read from the C# source, not guessed)
---------------------------------------------------------
C# `ContractMethodAttribute` (v3.10.1):

    Name, RequiredCallFlags, CpuFee, StorageFee, ActiveIn?, DeprecatedIn?

and it is declared `AllowMultiple = true` -- "the fees or requiredCallFlags may change
between hard forks" -- so one C# member may carry several attributes. The effective
metadata is therefore a *set of (ActiveIn, DeprecatedIn, CpuFee, flags) windows*.

C# `ContractMethodMetadata` derives the ABI type from the C# type via a fixed table
(`ToParameterType`), transcribed below. A leading `ApplicationEngine` / `DataCache`
parameter is stripped before the parameter list is built -- exactly what Rust encodes
as `engine` / `engine_only` in its handler tail.

Rust side: each method-table row is

    safe|unsafe "name", fee = <i64 expr>, flags = [A, B], params = [T, ...],
        returns = T [, active = HfX] [, deprecated = HfY]
        [, storage_fee = N] [, names = ["a", ...]] [=> engine|engine_only handler];

Rows appear in two shapes (with and without the `=> ... handler` tail); both are
handled. `safe` maps to C#'s `Safe = (RequiredCallFlags & ~CallFlags.ReadOnly) == 0`,
so the flag sets are comparable directly.

Usage
-----
    python scripts/verify-native-semantics.py
    python scripts/verify-native-semantics.py --json reports/native-semantics.json

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
CSHARP_DIR = "src/Neo/SmartContract/Native"

RUST_NATIVE_DIR = REPO_ROOT / "neo-core" / "src" / "smart_contract" / "native"

# contract -> (C# files, Rust method-table files)
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

# ---------------------------------------------------------------------------
# The C# type -> ContractParameterType table, transcribed from
# ContractMethodMetadata.ToParameterType (v3.10.1). Order matters: first match wins.
# ---------------------------------------------------------------------------
CSHARP_TYPE_MAP: list[tuple[str, str]] = [
    ("void", "Void"),
    ("bool", "Boolean"),
    ("sbyte", "Integer"),
    ("byte", "Integer"),
    ("short", "Integer"),
    ("ushort", "Integer"),
    ("int", "Integer"),
    ("uint", "Integer"),
    ("long", "Integer"),
    ("ulong", "Integer"),
    ("BigInteger", "Integer"),
    ("byte[]", "ByteArray"),
    ("string", "String"),
    ("UInt160", "Hash160"),
    ("UInt256", "Hash256"),
    ("ECPoint", "PublicKey"),
    ("Boolean", "Boolean"),
    ("Integer", "Integer"),
    ("ByteString", "ByteArray"),
    ("Buffer", "ByteArray"),
    ("Array", "Array"),
    ("Struct", "Array"),
    ("Map", "Map"),
    ("StackItem", "Any"),
    ("object", "Any"),
]

# A leading parameter of one of these types is the engine/snapshot and is stripped.
CSHARP_ENGINE_TYPES = {"ApplicationEngine"}
CSHARP_SNAPSHOT_TYPES = {"DataCache", "IReadOnlyStore", "IReadOnlyStoreView"}

# Types that appear in native contracts but are not primitive entries in the C#
# ToParameterType table. Each is resolved by the *tail rules* of ToParameterType,
# in the same precedence C# uses:
#     typeof(IInteroperable).IsAssignableFrom(type) -> Array
#     typeof(ISerializable).IsAssignableFrom(type)  -> ByteArray
#     type.IsArray                                  -> Array
#     type.IsEnum                                   -> Integer
#     otherwise                                     -> InteropInterface
#
# NOTE the precedence trap: a type implementing BOTH interfaces resolves to Array,
# because the IInteroperable check comes first. `ContractState` is exactly this case
# (it implements IInteroperableVerifiable : IInteroperable), so it is Array, not
# ByteArray -- do NOT "fix" it to ByteArray.
CSHARP_EXTRA_TYPE_MAP: dict[str, str] = {
    # enums
    "VMState": "Integer",
    "Role": "Integer",
    # IInteroperable (-> Array)
    # NOTE: StorageIterator : IIterator : IDisposable -- a bare chain that implements
    # neither IInteroperable nor ISerializable, so it resolves to InteropInterface too.
    "StorageIterator": "InteropInterface",
    # IIterator is a BARE interface (`IIterator : IDisposable`) -- it implements
    # neither IInteroperable nor ISerializable, so C#'s ToParameterType falls all the
    # way through to `return ContractParameterType.InteropInterface`. Do NOT map it to
    # Array; Rust correctly declares InteropInterface for these returns.
    "IIterator": "InteropInterface",
    "Iterator": "InteropInterface",
    "ContractState": "Array",
    "NeoAccountState": "Array",
    "NotaryAssisted": "Array",
    "Transaction": "Array",
    "Signer": "Array",
    "TrimmedBlock": "Array",
    # ISerializable but not IInteroperable (-> ByteArray).
    # Beware: Transaction / Signer / TrimmedBlock implement BOTH interfaces and the
    # IInteroperable check comes FIRST in C#'s ToParameterType, so they are Array.
    "OracleResponse": "ByteArray",
    "ECPoint": "PublicKey",  # explicit, listed for clarity
    # VM stack types (Neo.VM.Types.*)
    "InteropInterface": "InteropInterface",
    # non-generic task
    "ContractTask": "Void",
}

# C# enum types which map to Integer (ToParameterType: type.IsEnum -> Integer).
CSHARP_ENUM_TYPES = {
    "NamedCurveHash",
}


def fetch_text(url: str, offline: bool = False) -> str:
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


def csharp_type_to_param_type(cs_type: str) -> str | None:
    """Map a C# type token to a ContractParameterType name, or None if unknown."""
    t = re.sub(r"\s+", "", cs_type.strip())
    t = t.rstrip("?")
    if t.startswith("ContractTask"):
        inner = re.match(r"ContractTask<(.+)>", t)
        return csharp_type_to_param_type(inner.group(1)) if inner else "Void"
    for key, val in CSHARP_TYPE_MAP:
        if t == key:
            return val
    if t.endswith("[]"):
        return "Array"
    if t in CSHARP_ENUM_TYPES:
        return "Integer"
    if t in CSHARP_EXTRA_TYPE_MAP:
        return CSHARP_EXTRA_TYPE_MAP[t]
    return None  # caller reports it; never guess


# ---------------------------------------------------------------------------
# C# extraction
# ---------------------------------------------------------------------------

CSHARP_ATTR_RE = re.compile(r"\[ContractMethod(?:Attribute)?\s*\(([^)]*)\)\s*\]", re.S)
CSHARP_DECL_RE = re.compile(
    r"\b(?:public|private|protected|internal)\s+"
    r"(?:static\s+|override\s+|virtual\s+|abstract\s+|async\s+|new\s+|sealed\s+|extern\s+|partial\s+)*"
    r"[\w<>\[\](),.\?\s]+?\b(\w+)\s*(?=\(|\{)",
)
CSHARP_MODIFIERS = re.compile(
    r"\b(?:public|private|protected|internal|static|override|virtual|abstract|"
    r"async|new|sealed|extern|partial|readonly)\b"
)


def parse_csharp_attr_args(args: str) -> dict:
    out: dict = {
        "name": None,
        "cpu_fee": None,
        "storage_fee": None,
        "flags": None,
        "active_in": None,
        "deprecated_in": None,
    }
    # The C# ctor overloads are:
    #   ContractMethodAttribute(Hardfork activeIn)
    #   ContractMethodAttribute(Hardfork activeIn, Hardfork deprecatedIn)
    #   ContractMethodAttribute(bool isDeprecated = true, Hardfork deprecatedIn)
    # so a positional `true` must NOT be mistaken for an ActiveIn hardfork.
    positional_deprecated = bool(re.match(r"\s*true\s*,", args))
    # Positional Hardfork.HF_X: first -> ActiveIn (unless the leading arg was `true`,
    # which selects the `(bool isDeprecated, Hardfork deprecatedIn)` overload), else
    # -> DeprecatedIn.
    for m in re.finditer(r"\bHardfork\.(HF_\w+)", args):
        if out["active_in"] is None and not positional_deprecated:
            out["active_in"] = m.group(1)
        else:
            out["deprecated_in"] = m.group(1)
    for key, field in (
        ("CpuFee", "cpu_fee"),
        ("StorageFee", "storage_fee"),
        ("Name", "name"),
        ("RequiredCallFlags", "flags"),
        ("ActiveIn", "active_in"),
        ("DeprecatedIn", "deprecated_in"),
    ):
        m = re.search(rf"\b{key}\s*=\s*([^,]+?)(?=,\s*\w+\s*=|\s*$)", args)
        if not m:
            continue
        val = m.group(1).strip()
        if field in ("active_in", "deprecated_in"):
            hm = re.search(r"Hardfork\.(HF_\w+)", val)
            out[field] = hm.group(1) if hm else None
        elif field == "name":
            out[field] = val.strip('"')
        else:
            out[field] = val
    return out


def eval_cs_long(expr: str | None, consts: dict[str, str]) -> int | None:
    if expr is None:
        return None
    e = expr.strip()
    for _ in range(6):
        new = re.sub(
            r"\b([A-Za-z_]\w*)\b",
            lambda m: f"({consts[m.group(1)]})" if m.group(1) in consts else m.group(1),
            e,
        )
        if new == e:
            break
        e = new
    e = e.replace("L", "")
    if not re.fullmatch(r"[0-9\s<()|+*\-]+", e):
        return None
    try:
        return int(eval(e))
    except Exception:
        return None


def strip_param_attributes(param_str: str) -> str:
    """Remove `[...]` parameter attributes and inline `/* ... */` comments.

    Must run BEFORE comma-splitting: an attribute like `[MaxLength(MaxInputLength)]`
    contains parentheses that unbalance the nesting counter used for splitting, and
    an inline comment like `value/* in datoshi */` similarly carries no comma but can
    sit between the type and name. Removing both first keeps the split correct.

    The bracket body must be non-empty so the `[]` of `byte[]` survives.
    """
    param_str = re.sub(r"/\*.*?\*/", "", param_str, flags=re.S)
    return re.sub(r"\[[^\]]+\]", "", param_str)


def split_params(param_str: str) -> list[tuple[str, str]]:
    """Split a C# parameter list into (type, name), respecting <> and () nesting.

    `[]` is deliberately NOT treated as a nesting delimiter: in C# it is part of the
    type (`byte[]`, `Signer[]`), not a grouping construct. Tracking it would strip the
    brackets and turn `byte[]` into `byte` -> Integer, which is exactly the bug this
    avoids. Only `<>` and `()` nest.
    """
    param_str = strip_param_attributes(param_str)
    if not param_str.strip():
        return []
    parts, depth, cur = [], 0, ""
    for ch in param_str:
        if ch in "<(":
            depth += 1
        elif ch in ">)":
            depth = max(0, depth - 1)
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    out = []
    for p in parts:
        p = p.strip()
        # `strip_param_attributes` already removed [...] and inline comments; just drop
        # C# parameter modifiers that are not part of the ABI type.
        p = re.sub(r"\b(?:ref|out|in|params|this)\b", "", p).strip()
        if not p:
            continue
        m = re.match(r"(.+?)\s+(\w+)\s*$", p)
        if m:
            out.append((m.group(1).strip(), m.group(2)))
        else:
            out.append((p, ""))
    return out


def extract_param_list(sig: str, member: str) -> str | None:
    """Return the text inside the method's parameter parentheses.

    A regex like `member\\((.*?)\\)` stops at the FIRST `)`, which for
    `StrLen([MaxLength(MaxInputLength)] string str)` is the one closing `MaxLength(`,
    truncating the list. Scan with a depth counter instead.
    """
    m = re.search(re.escape(member) + r"\s*\(", sig)
    if not m:
        return None
    open_idx = sig.index("(", m.start())
    depth = 0
    for i in range(open_idx, len(sig)):
        ch = sig[i]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return sig[open_idx + 1 : i]
    return None


def extract_csharp_semantics(ref: str, offline: bool) -> dict[str, dict[str, list[dict]]]:
    result: dict[str, dict[str, list[dict]]] = {}
    for contract, (cs_files, _) in CONTRACTS.items():
        methods: dict[str, list[dict]] = {}
        for fname in cs_files:
            url = f"{CSHARP_BASE}/{ref}/{CSHARP_DIR}/{fname}"
            try:
                src = fetch_text(url, offline)
            except (RuntimeError, FileNotFoundError) as exc:
                print(f"warning: {exc}", file=sys.stderr)
                continue

            consts: dict[str, str] = {}
            for m in re.finditer(r"const\s+long\s+(\w+)\s*=\s*([^;]+);", src):
                consts[m.group(1)] = m.group(2).strip()

            for m in CSHARP_DECL_RE.finditer(src):
                member = m.group(1)
                # Collect the CONTIGUOUS run of [...] attribute blocks directly above the
                # declaration. C# deliberately stacks several ContractMethod attributes
                # (`AllowMultiple = true`) to express a fee/flag change at a hard-fork
                # boundary, e.g.
                #   [ContractMethod(true, Hardfork.HF_Echidna, ...)]   // deprecated in
                #   [ContractMethod(Hardfork.HF_Echidna, ...)]         // active in
                # Taking only the NEAREST one (`rfind`) silently drops a window.
                blocks = []
                cursor = m.start()
                while True:
                    p = cursor
                    while p > 0 and src[p - 1] in " \t\r\n":
                        p -= 1
                    if p == 0 or src[p - 1] != "]":
                        break
                    open_idx = src.rfind("[", 0, p - 1)
                    if open_idx == -1:
                        break
                    blocks.append(src[open_idx + 1 : p - 1])
                    cursor = open_idx
                blocks.reverse()
                if not blocks:
                    continue
                if all(not b.startswith("ContractMethod") for b in blocks):
                    continue
                # Extract just the argument list of each ContractMethod attribute.
                # `[ContractMethod]` with no parentheses is valid (all defaults), so the
                # bare form must yield an empty argument string rather than be dropped.
                attrs = []
                for b in blocks:
                    cm = re.match(r"ContractMethod(?:Attribute)?\s*\((.*)\)\s*$", b, re.S)
                    if cm:
                        attrs.append(cm.group(1))
                    elif re.fullmatch(r"ContractMethod(?:Attribute)?\s*", b):
                        attrs.append("")
                if not attrs:
                    continue

                sig_start = m.start()
                sig_end = min(
                    [x for x in (src.find("{", sig_start), src.find(";", sig_start)) if x != -1]
                    or [sig_start + 400]
                )
                sig = src[sig_start:sig_end]
                param_list = extract_param_list(sig, member)
                # The return type ends where the member name begins. `m` is the
                # declaration match whose group(1) IS the member name, so anchor on its
                # start relative to `sig` -- searching for "(" is wrong because a
                # parameter attribute like [MaxLength(MaxInputLength)] opens one earlier.
                name_pos = m.start(1) - sig_start
                ret_type = CSHARP_MODIFIERS.sub("", sig[:name_pos]).strip().rstrip("?").strip()
                params = split_params(param_list) if param_list is not None else []

                # Strip the leading engine/snapshot parameter.
                if params:
                    base_type = params[0][0].strip().split("<")[0].strip()
                    if base_type in CSHARP_ENGINE_TYPES or base_type in CSHARP_SNAPSHOT_TYPES:
                        params = params[1:]

                entity = {
                    "member": member,
                    "return_type": csharp_type_to_param_type(ret_type),
                    "raw_return": ret_type,
                    "params": [csharp_type_to_param_type(t) for t, _ in params],
                    "param_names": [n for _, n in params],
                    "windows": [],
                }
                for a in attrs:
                    parsed = parse_csharp_attr_args(a)
                    entity["windows"].append(
                        {
                            "name": parsed["name"],
                            "cpu_fee": eval_cs_long(parsed["cpu_fee"], consts),
                            "cpu_fee_expr": parsed["cpu_fee"],
                            "storage_fee": eval_cs_long(parsed["storage_fee"], consts),
                            "flags": parsed["flags"],
                            "active_in": parsed["active_in"],
                            "deprecated_in": parsed["deprecated_in"],
                        }
                    )
                eff = entity["windows"][0]["name"] or member
                eff = eff[0].lower() + eff[1:]
                entity["name"] = eff
                methods.setdefault(eff, []).append(entity)
        result[contract] = methods
    return result


# ---------------------------------------------------------------------------
# Rust extraction
# ---------------------------------------------------------------------------

def scan_rust_rows(text: str):
    """Yield (kind, name, body, handler_kind, handler) for every method row.

    Rows differ in shape: some end `returns = T;`, others
    `returns = T => engine handler;`. Scan semicolon-terminated statements and
    re-parse, so a lazy regex cannot stop early on the optional handler tail.
    """
    pos = 0
    row_start = re.compile(r'\b(safe|unsafe)\s+"')
    while True:
        m = row_start.search(text, pos)
        if not m:
            return
        end = text.find(";", m.end())
        if end == -1:
            return
        stmt = text[m.start() : end + 1]
        pos = end + 1
        kind = m.group(1)
        nm = re.match(r'\w+\s+"([^"]+)"\s*,', stmt)
        if not nm:
            continue
        name = nm.group(1)
        body = stmt[nm.end() :]
        hm = re.search(r"=>\s*(\w+)\s+(\w+)\s*;\s*$", body)
        if hm:
            handler_kind, handler = hm.group(1), hm.group(2)
            body = body[: hm.start()]
        else:
            handler_kind, handler = None, None
            body = body.rstrip(";")
        yield kind, name, body, handler_kind, handler


def split_top_level(body: str) -> list[str]:
    """Split `k = v, k = v, ...` on commas that are NOT inside [] or ()."""
    parts, depth, cur = [], 0, ""
    for ch in body:
        if ch in "[(":
            depth += 1
        elif ch in "])":
            depth = max(0, depth - 1)
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    return parts


def parse_rust_row(body: str) -> dict:
    """Parse a Rust method row body into its declared attributes.

    Values such as `flags = [STATES, ALLOW_NOTIFY]` and `params = [Integer, Integer]`
    contain commas INSIDE brackets, so a naive `[^,]+` value regex truncates them.
    Split on top-level commas first, then read each `key = value` pair.
    """
    fields: dict[str, str] = {}
    for chunk in split_top_level(body):
        m = re.match(r"\s*(\w+)\s*=\s*(.*?)\s*$", chunk, re.S)
        if m:
            fields[m.group(1)] = m.group(2).strip()

    def unlist(v: str | None) -> list[str]:
        if not v:
            return []
        v = v.strip()
        if v.startswith("[") and v.endswith("]"):
            v = v[1:-1]
        return [x.strip() for x in split_top_level(v) if x.strip()]

    params = unlist(fields.get("params"))
    names = [x.strip().strip('"') for x in unlist(fields.get("names"))]
    flags_raw = fields.get("flags")
    if flags_raw:
        flags_raw = flags_raw.strip()
        if flags_raw.startswith("[") and flags_raw.endswith("]"):
            flags_raw = flags_raw[1:-1]
    return {
        "fee_expr": fields.get("fee"),
        "flags": flags_raw,
        "params": params,
        "returns": fields.get("returns"),
        "active": fields.get("active"),
        "deprecated": fields.get("deprecated"),
        "storage_fee_expr": fields.get("storage_fee"),
        "names": names,
    }


def eval_rust_long(expr: str | None, consts: dict[str, str]) -> int | None:
    """Evaluate a Rust i64 expression against known consts, or None.

    Handles `Self::NAME` / `Type::NAME` qualification by stripping `::` BEFORE
    identifier resolution -- otherwise `Self` becomes an unresolvable token and the
    whole expression (e.g. `Self::CPU_FEE`) fails even though `CPU_FEE` is known.
    """
    if expr is None:
        return None
    # `Self::CPU_FEE` and `Type::NAME` are qualifications, not arithmetic. Drop the
    # left-hand side of each `::` (usually `Self`) and keep the constant name.
    e = re.sub(r"\b[A-Za-z_]\w*\s*::\s*", "", expr.strip())
    for _ in range(6):
        new = re.sub(
            r"\b([A-Za-z_]\w*)\b",
            lambda m: f"({consts[m.group(1)]})" if m.group(1) in consts else m.group(1),
            e,
        )
        if new == e:
            break
        e = new
    e = e.replace("_", "")
    if not re.fullmatch(r"[0-9\s<()|+*\-]+", e):
        return None
    try:
        return int(eval(e))
    except Exception:
        return None


def rust_hf(v: str | None) -> str | None:
    if not v:
        return None
    return "HF_" + v[2:] if v.startswith("Hf") else v


def extract_rust_semantics() -> dict[str, dict[str, list[dict]]]:
    result: dict[str, dict[str, list[dict]]] = {}
    for contract, (_, rs_files) in CONTRACTS.items():
        methods: dict[str, list[dict]] = {}
        consts: dict[str, str] = {}
        texts = []
        for rel in rs_files:
            p = RUST_NATIVE_DIR / rel
            if not p.exists():
                continue
            texts.append((p, p.read_text(encoding="utf-8")))
        # Constants may live beside the method table rather than in it (e.g.
        # `const CPU_FEE: i64 = 1 << 15;` sits in policy_contract/mod.rs while the table
        # is policy_contract/metadata.rs), so scan the whole contract directory for the
        # numeric constants the table references.
        scan_paths = {p for p, _ in texts}
        for p, _ in texts:
            scan_paths |= set(p.parent.glob("*.rs"))
        for p in sorted(scan_paths):
            try:
                t = p.read_text(encoding="utf-8")
            except OSError:
                continue
            for m in re.finditer(
                r"(?:pub\s+)?const\s+([A-Za-z_]\w*)\s*:\s*(?:i64|u32|u16|usize|i32)\s*=\s*([^;]+);",
                t,
            ):
                consts.setdefault(m.group(1), m.group(2).strip())
        for _, t in texts:
            for kind, name, body, handler_kind, handler in scan_rust_rows(t):
                row = parse_rust_row(body)
                row["kind"] = kind
                row["handler_kind"] = handler_kind
                row["handler"] = handler
                row["fee"] = eval_rust_long(row["fee_expr"], consts)
                row["storage_fee"] = eval_rust_long(row["storage_fee_expr"], consts)
                row["active"] = rust_hf(row["active"])
                row["deprecated"] = rust_hf(row["deprecated"])
                methods.setdefault(name, []).append(row)
        result[contract] = methods
    return result


# ---------------------------------------------------------------------------
# Comparison
# ---------------------------------------------------------------------------

# Composite CallFlags aliases, expanded to their primitive bits before comparison.
#
# Both implementations define these identically (C# CallFlags.cs / Rust
# neo-primitives/call_flags.rs):
#   STATES    = ReadStates | WriteStates
#   READ_ONLY = ReadStates | AllowCall
#   ALL       = STATES | AllowCall | AllowNotify
# so `All` on one side and `States | AllowCall | AllowNotify` on the other are the
# SAME required-flags value and must compare equal. Without this the script reports a
# phantom diff for every `transfer`.
FLAG_ALIASES: dict[str, set[str]] = {
    "states": {"readstates", "writestates"},
    "readonly": {"readstates", "allowcall"},
    "all": {"readstates", "writestates", "allowcall", "allownotify"},
}


def norm_flags(expr: str | None) -> tuple | None:
    """Canonical flag set. C# joins with `|`, Rust with `,`; accept both.

    `None` (no expression at all) and an empty set are DIFFERENT states: an absent
    flag expression means "not declared" while `CallFlags.None` means "the empty set".
    We collapse both to the empty tuple because C#'s `RequiredCallFlags` defaults to
    `CallFlags.None` (an enum with no value), so an attribute that omits the property
    and one that writes `CallFlags.None` are genuinely equivalent.
    """
    if expr is None:
        return ()
    out: set[str] = set()
    for t in re.split(r"[|+,]", expr):
        t = t.strip().replace("CallFlags.", "").replace("CallFlags::", "").lower().replace("_", "")
        if not t or t == "none":
            continue
        out |= FLAG_ALIASES.get(t, {t})
    return tuple(sorted(out))


def collapsed_windows_equivalent(cw: list, rw: list) -> bool:
    """True when C#'s multi-window chain is the same observable method as Rust's one.

    C# may implement a single contract-facing method as a chain of per-version
    overloads that together tile the whole height axis, e.g.

        [ContractMethod(true, Hardfork.HF_Gorgon, ..., Name = "destroy")]  // -> DeprecatedIn
        [ContractMethod(Hardfork.HF_Gorgon,        ..., Name = "destroy")]  // -> ActiveIn

    (DestroyV0 pre-Gorgon, DestroyV1 post-Gorgon). Rust instead writes one handler that
    branches on `is_hardfork_enabled(HfGorgon)` internally, so its table has a single
    window with no active/deprecated bounds. Observable ABI is the same: one `destroy`
    method, same fee/flags, present at every height.

    Accept exactly that shape, and nothing looser:
      * every C# window has the SAME (fee, flags) as the Rust window,
      * the C# windows chain: sorted by ActiveIn, each window's DeprecatedIn is the
        next window's ActiveIn (or None at the ends) -- i.e. they tile without a gap,
      * the chain starts with ActiveIn=None and ends with DeprecatedIn=None (present at
        every height),
      * Rust is a single window with ActiveIn=None and DeprecatedIn=None.

    Returns False for anything else, so a real window divergence is still reported.
    """
    if len(rw) != 1 or not cw:
        return False
    r_fee, r_flags = rw[0][2], rw[0][3]
    if rw[0][0] is not None or rw[0][1] is not None:
        return False
    if any(w[2] != r_fee or w[3] != r_flags for w in cw):
        return False
    ordered = sorted(cw, key=lambda x: (x[0] or "", x[1] or ""))
    if ordered[0][0] is not None or ordered[-1][1] is not None:
        return False
    for prev, nxt in zip(ordered, ordered[1:]):
        if prev[1] != nxt[0]:
            return False
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", help="write the machine-readable report here")
    parser.add_argument("--csharp-ref", default=CSHARP_REF)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()

    cs = extract_csharp_semantics(args.csharp_ref, args.offline)
    rs = extract_rust_semantics()

    diffs: list[dict] = []
    unresolved: list[str] = []
    total_cmp = 0
    windows_cmp = 0

    for contract in sorted(CONTRACTS):
        c_m, r_m = cs.get(contract, {}), rs.get(contract, {})
        for name in sorted(set(c_m) | set(r_m)):
            if name not in c_m or name not in r_m:
                diffs.append({"contract": contract, "method": name, "field": "presence",
                              "csharp": name in c_m, "rust": name in r_m})
                continue
            c_entities, r_list = c_m[name], r_m[name]
            # One C# member may carry several [ContractMethod] attributes (C# allows
            # it: fees/flags change between hard forks); flatten to windows.
            c_list = [w for e in c_entities for w in e["windows"]]
            c0, r0 = c_entities[0], r_list[0]
            total_cmp += 1
            if None in c0["params"] or c0["return_type"] is None:
                unresolved.append(
                    f"{contract}.{name}: csharp type not mapped "
                    f"({c0['params']} -> {c0['raw_return']})"
                )
            if c0["params"] != r0["params"]:
                diffs.append({"contract": contract, "method": name, "field": "params",
                              "csharp": c0["params"], "rust": r0["params"]})
            if c0["return_type"] != r0["returns"]:
                diffs.append({"contract": contract, "method": name, "field": "return_type",
                              "csharp": c0["return_type"], "raw_csharp": c0["raw_return"],
                              "rust": r0["returns"]})

            def wkey_cs(w):
                # C# `long CpuFee { get; init; }` defaults to 0 when the attribute omits
                # it, so an absent CpuFee and an explicit `CpuFee = 0` are the same fee.
                fee = w["cpu_fee"] if w["cpu_fee"] is not None else 0
                return (w["active_in"], w["deprecated_in"], fee, norm_flags(w["flags"]))

            def wkey_rs(w):
                return (w["active"], w["deprecated"], w["fee"], norm_flags(w["flags"]))

            cw = sorted((wkey_cs(w) for w in c_list), key=lambda x: (x[0] or "", x[1] or ""))
            rw = sorted((wkey_rs(w) for w in r_list), key=lambda x: (x[0] or "", x[1] or ""))

            if cw != rw and collapsed_windows_equivalent(cw, rw):
                # C# sometimes ships ONE observable method as a chain of per-version
                # overloads (e.g. `destroy` = DestroyV0 pre-Gorgon + DestroyV1 post-Gorgon,
                # both named "destroy"), while Rust implements a single handler that
                # branches on the hardfork internally. The observable ABI -- one method
                # with one fee/flags, present at every height -- is identical, so this is
                # NOT a divergence. Normalise before reporting.
                cw = rw

            windows_cmp += max(len(cw), len(rw))
            if cw != rw:
                diffs.append({"contract": contract, "method": name, "field": "windows",
                              "csharp": cw, "rust": rw})

    print(f"Native method semantic parity (C# {args.csharp_ref} vs Rust)")
    print("=" * 74)
    print(f"methods compared         : {total_cmp}")
    print(f"metadata windows compared: {windows_cmp}")
    if unresolved:
        print("-" * 74)
        print(f"UNRESOLVED C# types ({len(unresolved)}):")
        for u in unresolved:
            print(f"  {u}")
    print("-" * 74)
    if not diffs:
        print("params / return / cpu_fee / call_flags / active / deprecated: ALL MATCH")
    else:
        for d in diffs:
            print(f"DIFF {d['contract']}.{d['method']} [{d['field']}]")
            print(f"   csharp = {d['csharp']!r}")
            print(f"   rust   = {d['rust']!r}")
    print("-" * 74)
    ok = not diffs and not unresolved
    print(f"-> {'SEMANTIC PARITY' if ok else 'DIFFERENCES'}")

    if args.json:
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(
                {
                    "csharp_ref": args.csharp_ref,
                    "methods_compared": total_cmp,
                    "windows_compared": windows_cmp,
                    "differences": diffs,
                    "unresolved_csharp_types": unresolved,
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
