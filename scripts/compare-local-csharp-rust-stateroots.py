#!/usr/bin/env python3
import argparse
import base64
import json
import subprocess
import sys
import time


def build_auth_header(username: str | None, password: str | None) -> str | None:
    if not username:
        return None
    password = password or ""
    token = base64.b64encode(f"{username}:{password}".encode("utf-8")).decode("ascii")
    return f"Basic {token}"


def should_retry_rpc_error(error: dict) -> bool:
    return error.get("code") == -32001 and error.get("message") == "Too many requests"


def should_retry_curl_exit_code(code: int) -> bool:
    return code == 28


def chunk_ranges(start: int, end: int, chunk_size: int):
    if chunk_size <= 0:
        raise ValueError("chunk_size must be positive")
    current = start
    while current <= end:
        chunk_end = min(current + chunk_size - 1, end)
        yield (current, chunk_end)
        current = chunk_end + 1


def build_batch_requests(method: str, heights: list[int]):
    return [
        {"jsonrpc": "2.0", "id": idx + 1, "method": method, "params": [height]}
        for idx, height in enumerate(heights)
    ]


def rpc_call(url: str, method: str, params: list, auth_header: str | None = None):
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    )
    command = [
        "curl",
        "--compressed",
        "-sS",
        "--max-time",
        "20",
        "-H",
        "Content-Type: application/json",
        "-H",
        "Accept: application/json",
        "-H",
        "User-Agent: neo-rs-stateroot-compare/1.0",
        "-d",
        payload,
        url,
    ]
    if auth_header:
        command.extend(["-H", f"Authorization: {auth_header}"])
    for attempt in range(5):
        result = subprocess.run(command, check=False, capture_output=True, text=True)
        if result.returncode != 0:
            if should_retry_curl_exit_code(result.returncode) and attempt < 4:
                time.sleep(0.5 * (attempt + 1))
                continue
            raise subprocess.CalledProcessError(
                result.returncode, command, output=result.stdout, stderr=result.stderr
            )
        try:
            body = json.loads(result.stdout)
        except json.JSONDecodeError:
            if attempt < 4:
                time.sleep(0.5 * (attempt + 1))
                continue
            raise
        if "error" not in body:
            return body["result"]
        if should_retry_rpc_error(body["error"]) and attempt < 4:
            time.sleep(0.5 * (attempt + 1))
            continue
        raise RuntimeError(f"{method} returned error from {url}: {body['error']}")


def rpc_batch_call(url: str, requests: list[dict], auth_header: str | None = None):
    command = [
        "curl",
        "--compressed",
        "-sS",
        "--max-time",
        "20",
        "-H",
        "Content-Type: application/json",
        "-H",
        "Accept: application/json",
        "-H",
        "User-Agent: neo-rs-stateroot-compare/1.0",
        "-d",
        json.dumps(requests),
        url,
    ]
    if auth_header:
        command.extend(["-H", f"Authorization: {auth_header}"])

    for attempt in range(5):
        result = subprocess.run(command, check=False, capture_output=True, text=True)
        if result.returncode != 0:
            if should_retry_curl_exit_code(result.returncode) and attempt < 4:
                time.sleep(0.5 * (attempt + 1))
                continue
            raise subprocess.CalledProcessError(
                result.returncode, command, output=result.stdout, stderr=result.stderr
            )
        try:
            body = json.loads(result.stdout)
        except json.JSONDecodeError:
            if attempt < 4:
                time.sleep(0.5 * (attempt + 1))
                continue
            raise
        if isinstance(body, dict):
            if "error" in body:
                if should_retry_rpc_error(body["error"]) and attempt < 4:
                    time.sleep(0.5 * (attempt + 1))
                    continue
                raise RuntimeError(f"batch returned error from {url}: {body['error']}")
            raise RuntimeError(f"unexpected batch response shape from {url}: {body}")

        errors = [entry for entry in body if "error" in entry]
        if errors:
            retryable = all(should_retry_rpc_error(entry["error"]) for entry in errors)
            if retryable and attempt < 4:
                time.sleep(0.5 * (attempt + 1))
                continue
            raise RuntimeError(f"batch returned error from {url}: {errors[0]['error']}")

        ordered = sorted(body, key=lambda entry: entry["id"])
        return [entry["result"] for entry in ordered]

    raise RuntimeError(f"batch call exhausted retries for {url}")


def fetch_state_roots(
    url: str,
    start: int,
    end: int,
    auth_header: str | None = None,
    batch_size: int = 25,
):
    heights = list(range(start, end + 1))
    records = []
    for sub_start in range(0, len(heights), batch_size):
        sub_heights = heights[sub_start : sub_start + batch_size]
        roots = rpc_batch_call(
            url,
            build_batch_requests("getstateroot", sub_heights),
            auth_header,
        )
        for root in roots:
            records.append({"index": root["index"], "roothash": root["roothash"]})
    return records


def compare_records(local_records: list[dict], public_records: list[dict]):
    for local, public in zip(local_records, public_records):
        if local != public:
            return {
                "index": local["index"],
                "local": local,
                "public": public,
            }
    return None


def main():
    parser = argparse.ArgumentParser(
        description="Compare local Rust and public C# state roots over a height range."
    )
    parser.add_argument("--rust-rpc", required=True)
    parser.add_argument("--csharp-rpc", required=True)
    parser.add_argument("--start-height", type=int, default=0)
    parser.add_argument("--end-height", type=int, required=True)
    parser.add_argument("--chunk-size", type=int, default=1000)
    parser.add_argument("--batch-size", type=int, default=25)
    parser.add_argument("--rust-rpc-user")
    parser.add_argument("--rust-rpc-pass")
    args = parser.parse_args()

    auth_header = build_auth_header(args.rust_rpc_user, args.rust_rpc_pass)

    for chunk_start, chunk_end in chunk_ranges(
        args.start_height, args.end_height, args.chunk_size
    ):
        local_records = fetch_state_roots(
            args.rust_rpc, chunk_start, chunk_end, auth_header, args.batch_size
        )
        public_records = fetch_state_roots(
            args.csharp_rpc, chunk_start, chunk_end, None, args.batch_size
        )
        mismatch = compare_records(local_records, public_records)
        if mismatch is not None:
            print(f"FAIL state root mismatch at height {mismatch['index']}")
            print("local:", json.dumps(mismatch["local"], sort_keys=True))
            print("public:", json.dumps(mismatch["public"], sort_keys=True))
            return 1
        print(f"OK   state roots matched for {chunk_start}..{chunk_end}")

    print("All compared state roots matched.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
