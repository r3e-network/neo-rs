#!/usr/bin/env python3
import argparse
import base64
import json
import struct
import sys
import time
import urllib.error
import urllib.request


def eprint(message: str) -> None:
    print(message, file=sys.stderr, flush=True)


def rpc_post(url: str, payload, timeout: float):
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            "Accept-Encoding": "identity",
            "User-Agent": "neo-rs-acc-builder/1.0",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        raw = resp.read()
    return json.loads(raw.decode("utf-8"))


def single_rpc_call(url: str, method: str, params: list, timeout: float):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    response = rpc_post(url, payload, timeout)
    if "error" in response:
        raise RuntimeError(f"{method} returned error: {response['error']}")
    return response["result"]


def batch_get_blocks(url: str, heights: list[int], timeout: float):
    payload = [
        {"jsonrpc": "2.0", "id": height, "method": "getblock", "params": [height, 0]}
        for height in heights
    ]

    try:
        response = rpc_post(url, payload, timeout)
        if not isinstance(response, list):
            raise RuntimeError("batch response was not a JSON array")
        by_id = {}
        for item in response:
            if "error" in item:
                raise RuntimeError(f"batch getblock({item.get('id')}) error: {item['error']}")
            by_id[item["id"]] = item["result"]
        return [by_id[height] for height in heights]
    except Exception as exc:
        eprint(f"batch request failed, falling back to singles: {exc}")
        return [
            single_rpc_call(url, "getblock", [height, 0], timeout)
            for height in heights
        ]


def decode_block_blob(value: str, height: int) -> bytes:
    try:
        return base64.b64decode(value, validate=True)
    except Exception as exc:
        raise RuntimeError(f"getblock({height}, 0) did not return valid base64: {exc}") from exc


def build_acc(url: str, start: int, end: int, output_path: str, batch_size: int, timeout: float):
    if start < 0 or end < start:
        raise ValueError(f"invalid range start={start} end={end}")
    if batch_size <= 0:
        raise ValueError("batch-size must be positive")

    total = end - start + 1
    written = 0
    started_at = time.time()

    with open(output_path, "wb") as f:
        f.write(struct.pack("<I", start))
        f.write(struct.pack("<I", total))

        for batch_start in range(start, end + 1, batch_size):
            batch_end = min(end, batch_start + batch_size - 1)
            heights = list(range(batch_start, batch_end + 1))
            encoded_blocks = batch_get_blocks(url, heights, timeout)

            for height, encoded in zip(heights, encoded_blocks):
                block_bytes = decode_block_blob(encoded, height)
                f.write(struct.pack("<I", len(block_bytes)))
                f.write(block_bytes)
                written += 1

            elapsed = max(time.time() - started_at, 0.001)
            rate = written / elapsed
            eprint(
                f"wrote blocks {batch_start}-{batch_end} "
                f"({written}/{total}, {rate:.1f} blocks/s)"
            )


def parse_args():
    parser = argparse.ArgumentParser(
        description="Build a Neo .acc file from JSON-RPC getblock(height, 0) responses."
    )
    parser.add_argument("--rpc", required=True, help="RPC endpoint URL")
    parser.add_argument("--start", required=True, type=int, help="Starting block height")
    parser.add_argument("--end", required=True, type=int, help="Ending block height (inclusive)")
    parser.add_argument("--output", required=True, help="Output .acc file path")
    parser.add_argument(
        "--batch-size",
        type=int,
        default=50,
        help="Number of getblock requests per batch attempt (default: 50)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=20.0,
        help="HTTP timeout in seconds (default: 20)",
    )
    return parser.parse_args()


def main():
    args = parse_args()
    build_acc(
        url=args.rpc,
        start=args.start,
        end=args.end,
        output_path=args.output,
        batch_size=args.batch_size,
        timeout=args.timeout,
    )


if __name__ == "__main__":
    main()
