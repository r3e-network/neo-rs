#!/usr/bin/env python3
import argparse
import copy
import gzip
import json
import sys
import urllib.request


def rpc_call(url: str, method: str, params: list):
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    ).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=payload,
        headers={"Content-Type": "application/json", "Accept-Encoding": "identity"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw = resp.read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    parsed = json.loads(raw.decode("utf-8"))
    return parsed.get("result") if "error" not in parsed else {"error": parsed["error"]}


def normalize(value):
    if isinstance(value, dict):
        out = {}
        for key, inner in value.items():
            if key in {"id", "session"} and isinstance(inner, str):
                out[key] = "<opaque-id>"
            else:
                out[key] = normalize(inner)
        return out
    if isinstance(value, list):
        return [normalize(item) for item in value]
    return value


def compare(rust_rpc: str, csharp_rpc: str):
    native_contracts = rpc_call(csharp_rpc, "getnativecontracts", [])
    if not isinstance(native_contracts, list):
        raise RuntimeError("getnativecontracts did not return an array")

    checks = []
    for contract in native_contracts:
        name = contract["manifest"]["name"]
        script_hash = contract["hash"]
        for method in contract["manifest"]["abi"]["methods"]:
            if not method.get("safe"):
                continue
            if method.get("parameters"):
                continue
            checks.append((name, script_hash, method["name"]))

    failures = []
    for contract_name, script_hash, method_name in checks:
        rust = normalize(rpc_call(rust_rpc, "invokefunction", [script_hash, method_name, []]))
        csharp = normalize(
            rpc_call(csharp_rpc, "invokefunction", [script_hash, method_name, []])
        )
        label = f"{contract_name}.{method_name}"
        if rust == csharp:
            print(f"OK   {label}")
        else:
            print(f"FAIL {label}")
            failures.append((label, rust, csharp))

    if failures:
        print("")
        for label, rust, csharp in failures:
            print(f"Mismatch: {label}")
            print("rust:")
            print(json.dumps(rust, indent=2, sort_keys=True))
            print("csharp:")
            print(json.dumps(csharp, indent=2, sort_keys=True))
            print("")
        return 1

    print("")
    print(f"All {len(checks)} zero-argument safe native methods matched.")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Compare zero-argument safe native-contract methods between local Rust and C# Neo nodes."
    )
    parser.add_argument("--rust-rpc", required=True)
    parser.add_argument("--csharp-rpc", required=True)
    args = parser.parse_args()
    sys.exit(compare(args.rust_rpc, args.csharp_rpc))


if __name__ == "__main__":
    main()
