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
        raise RuntimeError(f"{method} returned error from {url}: {payload['error']}")
    return payload["result"]


def connected_peer_ports(peers_result):
    connected = peers_result.get("connected", [])
    return sorted(entry.get("port") for entry in connected if isinstance(entry, dict))


def compare(rust_rpc: str, csharp_rpc: str, rust_p2p_port: int, csharp_p2p_port: int):
    rust_count = rpc_call(rust_rpc, "getconnectioncount", [])
    csharp_count = rpc_call(csharp_rpc, "getconnectioncount", [])
    rust_peers = rpc_call(rust_rpc, "getpeers", [])
    csharp_peers = rpc_call(csharp_rpc, "getpeers", [])

    rust_ports = connected_peer_ports(rust_peers)
    csharp_ports = connected_peer_ports(csharp_peers)

    failures = []
    if rust_count < 1:
        failures.append(f"Rust node reports getconnectioncount={rust_count}")
    if csharp_count < 1:
        failures.append(f"C# node reports getconnectioncount={csharp_count}")
    if csharp_p2p_port not in rust_ports:
        failures.append(
            f"Rust node did not report C# peer port {csharp_p2p_port}; connected={rust_ports}"
        )
    if rust_p2p_port not in csharp_ports:
        failures.append(
            f"C# node did not report Rust peer port {rust_p2p_port}; connected={csharp_ports}"
        )

    if failures:
        for failure in failures:
            print(f"FAIL {failure}")
        return 1

    print("OK   getconnectioncount / getpeers handshake parity")
    print(f"rust connected ports: {rust_ports}")
    print(f"csharp connected ports: {csharp_ports}")
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Check localhost Rust/C# Neo P2P handshake via RPC peer views."
    )
    parser.add_argument("--rust-rpc", required=True)
    parser.add_argument("--csharp-rpc", required=True)
    parser.add_argument("--rust-p2p-port", required=True, type=int)
    parser.add_argument("--csharp-p2p-port", required=True, type=int)
    args = parser.parse_args()
    sys.exit(
        compare(
            args.rust_rpc,
            args.csharp_rpc,
            args.rust_p2p_port,
            args.csharp_p2p_port,
        )
    )


if __name__ == "__main__":
    main()
