#!/usr/bin/env python3
"""Extract an inclusive block range from a Neo offline .acc file as Base64."""

from __future__ import annotations

import argparse
import base64
import os
import struct
from pathlib import Path


INDEX_MAGIC = b"NEOACCIX"
INDEX_HEADER = struct.Struct("<8sIIQQ")
OFFSET = struct.Struct("<Q")


def offset_index_path(acc: Path) -> Path:
    return acc.with_name(acc.name + ".offsets")


def build_offset_index(acc: Path, index: Path) -> None:
    """Index each block's length field, validating the complete ACC structure."""
    temporary = index.with_name(index.name + f".{os.getpid()}.tmp")
    try:
        with acc.open("rb") as stream, temporary.open("xb") as output:
            declared_start, declared_count = struct.unpack(
                "<II", read_exact(stream, 8, "ACC header")
            )
            if declared_count == 0:
                raise ValueError("ACC index requires at least one block")
            stat = acc.stat()
            output.write(
                INDEX_HEADER.pack(
                    INDEX_MAGIC,
                    declared_start,
                    declared_count,
                    stat.st_size,
                    stat.st_mtime_ns,
                )
            )
            for height in range(declared_start, declared_start + declared_count):
                output.write(OFFSET.pack(stream.tell()))
                size = struct.unpack(
                    "<I", read_exact(stream, 4, f"block {height} length")
                )[0]
                stream.seek(size, os.SEEK_CUR)
            if stream.tell() > stat.st_size:
                raise ValueError(f"truncated ACC payload at height {declared_start + declared_count - 1}")
        os.replace(temporary, index)
    finally:
        temporary.unlink(missing_ok=True)


def seek_to_start(stream, acc: Path, start: int) -> None:
    index = offset_index_path(acc)
    stat = acc.stat()
    if not index.is_file():
        build_offset_index(acc, index)
    with index.open("rb") as offsets:
        magic, declared_start, declared_count, size, mtime_ns = INDEX_HEADER.unpack(
            read_exact(offsets, INDEX_HEADER.size, "ACC index header")
        )
        if magic != INDEX_MAGIC or size != stat.st_size or mtime_ns != stat.st_mtime_ns:
            raise ValueError("stale or invalid ACC offset index; delete it to rebuild")
        if start < declared_start or start >= declared_start + declared_count:
            raise ValueError(f"indexed start {start} outside ACC range")
        offsets.seek(INDEX_HEADER.size + (start - declared_start) * OFFSET.size)
        stream.seek(OFFSET.unpack(read_exact(offsets, OFFSET.size, "ACC block offset"))[0])


def read_exact(stream, size: int, label: str) -> bytes:
    data = stream.read(size)
    if len(data) != size:
        raise ValueError(f"truncated {label}: expected {size} bytes, got {len(data)}")
    return data


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("acc", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("start", type=int)
    parser.add_argument("end", type=int)
    args = parser.parse_args()
    if args.start < 0 or args.end < args.start:
        parser.error("require 0 <= start <= end")

    args.output.mkdir(parents=True, exist_ok=True)
    with args.acc.open("rb") as stream:
        declared_start, declared_count = struct.unpack(
            "<II", read_exact(stream, 8, "ACC header")
        )
        declared_end = declared_start + declared_count - 1
        if args.start < declared_start or args.end > declared_end:
            raise ValueError(
                f"requested {args.start}..={args.end} outside "
                f"ACC range {declared_start}..={declared_end}"
            )

        seek_to_start(stream, args.acc, args.start)
        extracted = 0
        for height in range(args.start, args.end + 1):
            size = struct.unpack(
                "<I", read_exact(stream, 4, f"block {height} length")
            )[0]
            payload = read_exact(stream, size, f"block {height} payload")
            encoded = base64.b64encode(payload) + b"\n"
            path = args.output / f"{height}.b64"
            if not path.exists() or path.read_bytes() != encoded:
                path.write_bytes(encoded)
            extracted += 1

    expected = args.end - args.start + 1
    if extracted != expected:
        raise ValueError(f"extracted {extracted} blocks, expected {expected}")
    print(
        f"acc_range={declared_start}..={declared_end} "
        f"extracted_range={args.start}..={args.end} count={extracted}"
    )


if __name__ == "__main__":
    main()
