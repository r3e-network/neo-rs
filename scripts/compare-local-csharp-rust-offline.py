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
    payload = json.loads(raw.decode("utf-8"))
    if "error" in payload:
        return {"error": payload["error"]}
    return payload["result"]


def normalize_native_contracts(items):
    return [
        {
            "id": item["id"],
            "hash": item["hash"],
            "name": item["manifest"]["name"],
            "updatecounter": item["updatecounter"],
        }
        for item in items
    ]


def native_contract_state(url: str, name: str):
    return rpc_call(url, "getcontractstate", [name])


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


def compare(rust_url: str, csharp_url: str):
    checks = [
        ("getversion.protocol", lambda url: rpc_call(url, "getversion", [])["protocol"]),
        ("getblockcount", lambda url: rpc_call(url, "getblockcount", [])),
        ("getblockhash(0)", lambda url: rpc_call(url, "getblockhash", [0])),
        ("getbestblockhash", lambda url: rpc_call(url, "getbestblockhash", [])),
        ("getblockheadercount", lambda url: rpc_call(url, "getblockheadercount", [])),
        ("getconnectioncount", lambda url: rpc_call(url, "getconnectioncount", [])),
        ("getpeers", lambda url: rpc_call(url, "getpeers", [])),
        ("getrawmempool", lambda url: rpc_call(url, "getrawmempool", [])),
        ("getrawmempool(true)", lambda url: rpc_call(url, "getrawmempool", [True])),
        ("getcommittee", lambda url: rpc_call(url, "getcommittee", [])),
        ("getnextblockvalidators", lambda url: rpc_call(url, "getnextblockvalidators", [])),
        ("getcandidates", lambda url: rpc_call(url, "getcandidates", [])),
        (
            "invokefunction(NeoToken.getCandidates)",
            lambda url: rpc_call(
                url,
                "invokefunction",
                ["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5", "getCandidates", []],
            ),
        ),
        (
            "invokefunction(NeoToken.getAllCandidates)",
            lambda url: rpc_call(
                url,
                "invokefunction",
                [
                    "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5",
                    "getAllCandidates",
                    [],
                ],
            ),
        ),
        (
            "getnativecontracts",
            lambda url: normalize_native_contracts(rpc_call(url, "getnativecontracts", [])),
        ),
        (
            "policy.getters",
            lambda url: {
                method: rpc_call(
                    url,
                    "invokefunction",
                    ["0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b", method, []],
                )["stack"][0]["value"]
                for method in ("getFeePerByte", "getExecFeeFactor", "getStoragePrice")
            },
        ),
        ("getblock(0,1)", lambda url: rpc_call(url, "getblock", [0, 1])),
        ("getblockheader(0,1)", lambda url: rpc_call(url, "getblockheader", [0, 1])),
        (
            "getcontractstate(CryptoLib)",
            lambda url: rpc_call(url, "getcontractstate", ["CryptoLib"]),
        ),
        (
            "getcontractstate(all natives)",
            lambda url: {
                name: native_contract_state(url, name)
                for name in (
                    "ContractManagement",
                    "StdLib",
                    "CryptoLib",
                    "LedgerContract",
                    "NeoToken",
                    "GasToken",
                    "PolicyContract",
                    "RoleManagement",
                    "OracleContract",
                )
            },
        ),
        (
            "invokescript(push1)",
            lambda url: rpc_call(url, "invokescript", ["EQ=="]),
        ),
        (
            "invokefunction(NeoToken.totalSupply)",
            lambda url: rpc_call(
                url,
                "invokefunction",
                ["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5", "totalSupply", []],
            ),
        ),
        (
            "invokefunction(NeoToken.readonly)",
            lambda url: {
                method: rpc_call(
                    url,
                    "invokefunction",
                    ["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5", method, []],
                )
                for method in ("symbol", "decimals", "totalSupply")
            },
        ),
        (
            "invokefunction(GasToken.readonly)",
            lambda url: {
                method: rpc_call(
                    url,
                    "invokefunction",
                    ["0xd2a4cff31913016155e38e474a2c06d08be276cf", method, []],
                )
                for method in ("symbol", "decimals", "totalSupply")
            },
        ),
        (
            "invokefunction(CryptoLib.bytearray)",
            lambda url: {
                method: rpc_call(
                    url,
                    "invokefunction",
                    [
                        "0x726cb6e0cd8628a1350a611384688911ab75f51b",
                        method,
                        [{"type": "ByteArray", "value": "AQI="}],
                    ],
                )
                for method in ("sha256", "ripemd160")
            },
        ),
        (
            "invokefunction(fault.exceptions)",
            lambda url: {
                "Policy.getAttributeFee(16)": rpc_call(
                    url,
                    "invokefunction",
                    [
                        "0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b",
                        "getAttributeFee",
                        [{"type": "Integer", "value": "16"}],
                    ],
                ),
                "Crypto.keccak256(0102)": rpc_call(
                    url,
                    "invokefunction",
                    [
                        "0x726cb6e0cd8628a1350a611384688911ab75f51b",
                        "keccak256",
                        [{"type": "ByteArray", "value": "AQI="}],
                    ],
                ),
            },
        ),
        (
            "validateaddress(samples)",
            lambda url: {
                address: rpc_call(url, "validateaddress", [address])
                for address in (
                    "NVGUzpQKGY7j11CLQY7PKr846HNDHC4atB",
                    "Nb2oJkQSV7WcK9QGmW6v7M3cUGa2Q8XxVY",
                    "notanaddress",
                )
            },
        ),
        (
            "invalid.request.shapes",
            lambda url: {
                "sendrawtransaction.invalid_base64": rpc_call(
                    url, "sendrawtransaction", ["not-base64"]
                ),
                "submitblock.invalid_base64": rpc_call(url, "submitblock", ["not-base64"]),
                "getblock.null": rpc_call(url, "getblock", [None]),
                "getblockheader.null": rpc_call(url, "getblockheader", [None]),
                "getcontractstate.invalid_name": rpc_call(
                    url, "getcontractstate", ["InvalidContractName"]
                ),
                "getcontractstate.invalid_hash": rpc_call(
                    url, "getcontractstate", ["0xInvalidHashString"]
                ),
                "invokescript.invalid_base64": rpc_call(url, "invokescript", ["not-base64"]),
            },
        ),
    ]

    failures = []
    for label, fn in checks:
        rust = normalize(fn(rust_url))
        csharp = normalize(fn(csharp_url))
        if rust != csharp:
            failures.append((label, rust, csharp))
            print(f"FAIL {label}")
        else:
            print(f"OK   {label}")

    if failures:
        print("")
        for label, rust, csharp in failures:
            print(f"Mismatch: {label}")
            print("rust:")
            print(json.dumps(rust, indent=2, sort_keys=True))
            print("csharp:")
            print(json.dumps(csharp, indent=2, sort_keys=True))
        return 1

    print("")
    print("All checked offline RPC methods matched.")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Compare offline Rust and C# Neo RPC nodes on genesis-safe methods."
    )
    parser.add_argument("--rust-rpc", required=True, help="Rust RPC URL")
    parser.add_argument("--csharp-rpc", required=True, help="C# RPC URL")
    args = parser.parse_args()
    sys.exit(compare(args.rust_rpc, args.csharp_rpc))


if __name__ == "__main__":
    main()
