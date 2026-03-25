#!/usr/bin/env python3
import argparse
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
    return json.loads(raw.decode("utf-8"))


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


def dummy_parameter(param_type: str):
    if param_type == "Integer":
        return {"type": "Integer", "value": "0"}
    if param_type == "Hash160":
        return {"type": "Hash160", "value": "0x0000000000000000000000000000000000000000"}
    if param_type == "Hash256":
        return {
            "type": "Hash256",
            "value": "0x0000000000000000000000000000000000000000000000000000000000000000",
        }
    if param_type == "ByteArray":
        return {"type": "ByteArray", "value": "AQI="}
    if param_type == "String":
        return {"type": "String", "value": ""}
    if param_type == "Boolean":
        return {"type": "Boolean", "value": False}
    if param_type == "PublicKey":
        return {
            "type": "PublicKey",
            "value": "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
        }
    return None


def compare(rust_rpc: str, csharp_rpc: str):
    contracts = rpc_call(csharp_rpc, "getnativecontracts", []).get("result")
    if not isinstance(contracts, list):
        raise RuntimeError("getnativecontracts did not return a result array")

    failures = []
    checked = 0

    for contract in contracts:
        name = contract["manifest"]["name"]
        script_hash = contract["hash"]
        for method in contract["manifest"]["abi"]["methods"]:
            if not method.get("safe"):
                continue
            params_meta = method.get("parameters", [])
            if not (1 <= len(params_meta) <= 4):
                continue

            params = []
            for meta in params_meta:
                dummy = dummy_parameter(meta["type"])
                if dummy is None:
                    params = None
                    break
                params.append(dummy)
            if params is None:
                continue

            checked += 1
            rust = normalize(
                rpc_call(rust_rpc, "invokefunction", [script_hash, method["name"], params])
            )
            csharp = normalize(
                rpc_call(csharp_rpc, "invokefunction", [script_hash, method["name"], params])
            )
            label = f"{name}.{method['name']}({len(params_meta)})"
            if rust == csharp:
                print(f"OK   {label}")
            else:
                print(f"FAIL {label}")
                failures.append((label, params, rust, csharp))

    if failures:
        print("")
        for label, params, rust, csharp in failures:
            print(f"Mismatch: {label}")
            print(f"params: {json.dumps(params, indent=2)}")
            print("rust:")
            print(json.dumps(rust, indent=2, sort_keys=True))
            print("csharp:")
            print(json.dumps(csharp, indent=2, sort_keys=True))
            print("")
        return 1

    print("")
    print(f"All {checked} sampled one-to-four-argument safe native methods matched.")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Compare sampled one/two-argument safe native methods between local Rust and C# Neo nodes."
    )
    parser.add_argument("--rust-rpc", required=True)
    parser.add_argument("--csharp-rpc", required=True)
    args = parser.parse_args()
    sys.exit(compare(args.rust_rpc, args.csharp_rpc))


if __name__ == "__main__":
    main()
