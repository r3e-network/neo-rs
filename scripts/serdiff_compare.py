#!/usr/bin/env python3
"""Cross-reference Rust roundtrip JSON and C# Neo 3.10.1 cross-check JSON.

Exit 0 = all vectors present on both sides and all passed, 1 = any mismatch.
"""
import json, sys, collections

rust_path, csharp_path, out_path = sys.argv[1:4]
rust = json.load(open(rust_path))
csh  = json.load(open(csharp_path))

r_by = {r["name"]: r for r in rust["results"]}
c_by = {r["name"]: r for r in csh["results"]}

names_r = set(r_by)
names_c = set(c_by)
missing_c = sorted(names_r - names_c)
missing_r = sorted(names_c - names_r)
only_c_not_rust = [n for n in missing_r]
# C# results only names the vectors we asked it to decode from the same file,
# so the union should be identical. Report any drift.

summary = {
    "rust_total": rust["total"],
    "rust_roundtrip_pass": rust["roundtrip_pass"],
    "rust_roundtrip_fail": rust["roundtrip_fail"],
    "rust_failures": rust["failures"],
    "csharp_total": csh["total"],
    "csharp_matched": csh["matched"],
    "csharp_mismatched": csh["mismatched"],
    "csharp_errors": [n for n in sorted(c_by) if "error" in c_by[n]],
    "names_present_on_both": sorted(names_r & names_c),
    "missing_on_csharp_side": missing_c,
    "present_only_on_csharp_side": missing_r,
    "byte_compatible": (rust["roundtrip_pass"] == rust["total"]
                        and csh["matched"] == csh["total"]
                        and not missing_c and not missing_r),
}
json.dump(summary, open(out_path, "w"), indent=2)
bad = False
if summary["byte_compatible"]:
    print(f"ALL COMPATIBLE: Rust roundtrip {rust['roundtrip_pass']}/{rust['total']}, C# byte-match {csh['matched']}/{csh['total']}")
else:
    bad = True
    print(f"NOT COMPATIBLE: rust_fail={rust['roundtrip_fail']} csharp_mismatch={csh['mismatched']} "
          f"missing_c={missing_c} only_c={missing_r}", file=sys.stderr)
print(json.dumps(summary))
sys.exit(1 if bad else 0)
