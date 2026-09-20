#!/usr/bin/env python3
"""Cross-reference the Rust and C# signature-verification outcomes.

Exit 0 = every signature witness present on both sides and every outcome agrees.
Exit 1 = any mismatch or missing entry.
"""
import json
import sys


def main() -> int:
    rust_path, csharp_path, out_path = sys.argv[1:4]
    rust = json.load(open(rust_path))
    csh = json.load(open(csharp_path))

    r_by = {}
    for tx in rust["results"]:
        for wit in tx.get("witnesses", []):
            if wit.get("kind") == "signature":
                r_by[wit["signature"]] = wit["rust_verify"]
    c_by = {r["signature"]: r["csharp_verify"] for r in csh["results"]}

    signatures = sorted(set(r_by) | set(c_by))
    mismatches = [
        sig for sig in signatures
        if sig not in r_by or sig not in c_by or r_by[sig] != c_by[sig]
    ]

    summary = {
        "rust_total": rust["total_signature_witnesses"],
        "rust_verify_ok": rust["rust_verify_ok"],
        "csharp_total": csh["total_signature_witnesses"],
        "csharp_agree": csh["agree"],
        "both_verify": csh["both_verify"],
        "signature_outcomes_agree": not mismatches,
        "mismatched_signatures": mismatches[:5],
    }
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(summary, handle, indent=2)

    ok = summary["signature_outcomes_agree"] and summary["csharp_agree"] == summary["rust_total"]
    print(json.dumps(summary))
    print("ALL AGREE" if ok else "MISMATCH")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
