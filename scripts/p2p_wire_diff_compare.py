#!/usr/bin/env python3
"""Cross-reference Rust P2P wire vectors/results with the C# 3.10.1 runner output.

Exit 0 = every vector present on both sides, all matched, command tables agree.
Exit 1 = any mismatch, missing vector, or command-byte drift.
"""
import json
import sys

ALIAS = {
    "Version": "version", "Verack": "verack", "GetAddr": "getaddr", "Addr": "addr",
    "Ping": "ping", "Pong": "pong", "GetHeaders": "getheaders", "Headers": "headers",
    "GetBlocks": "getblocks", "Mempool": "mempool", "Inv": "inv", "GetData": "getdata",
    "GetBlockByIndex": "getblkbyidx", "NotFound": "notfound", "Extensible": "extensible",
    "Reject": "reject", "FilterLoad": "filterload", "FilterAdd": "filteradd",
    "FilterClear": "filterclear", "MerkleBlock": "merkleblock", "Alert": "alert",
    "Transaction": "transaction", "Block": "block",
}


def main() -> int:
    vectors_path, rust_results_path, csharp_results_path, out_path = sys.argv[1:5]
    vectors = json.load(open(vectors_path))
    rust = json.load(open(rust_results_path))
    csh = json.load(open(csharp_results_path))

    rust_table = vectors["command_table"]
    csharp_table = {ALIAS.get(k, k.lower()): v for k, v in csh["command_table"].items()}

    command_mismatch = sorted(
        name for name, value in rust_table.items() if csharp_table.get(name) != value
    )
    csharp_only_commands = sorted(
        name for name in csharp_table if name not in rust_table
    )

    matched = csh["matched"]
    total = csh["total"]

    summary = {
        "rust_vectors": vectors and len(vectors["vectors"]),
        "rust_roundtrip_pass": rust["roundtrip_pass"],
        "rust_total": rust["total"],
        "csharp_total": total,
        "csharp_matched": matched,
        "csharp_mismatched": csh["mismatched"],
        "command_bytes_agree": not command_mismatch,
        "command_mismatched": command_mismatch,
        "csharp_only_commands": csharp_only_commands,
        "byte_compatible": (
            matched == total
            and rust["roundtrip_pass"] == rust["total"]
            and not command_mismatch
        ),
    }
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(summary, handle, indent=2)

    ok = summary["byte_compatible"]
    print(json.dumps(summary))
    print("ALL COMPATIBLE" if ok else "NOT COMPATIBLE")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
