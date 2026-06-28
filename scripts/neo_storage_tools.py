"""Small Neo storage/RPC helpers used by validation scripts."""

from __future__ import annotations

import base64
import gzip
import hashlib
import json
import urllib.request
from typing import Any


DEFAULT_REFERENCE_RPC = "http://seed1.neo.org:10332"
DEFAULT_ADDRESS_VERSION = 0x35
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
NEP17_ACCOUNT_PREFIX = 20
STACK_ITEM_INTEGER = 0x21
STACK_ITEM_STRUCT = 0x41
GAS_HASH = "0xd2a4cff31913016155e38e474a2c06d08be276cf"


def base58check_decode(value: str) -> bytes:
    number = 0
    for char in value:
        try:
            digit = BASE58_ALPHABET.index(char)
        except ValueError as exc:
            raise ValueError(f"invalid Base58 character: {char!r}") from exc
        number = number * 58 + digit

    decoded = number.to_bytes((number.bit_length() + 7) // 8, "big")
    decoded = b"\x00" * (len(value) - len(value.lstrip("1"))) + decoded
    if len(decoded) < 4:
        raise ValueError("invalid Base58Check payload: missing checksum")
    payload, checksum = decoded[:-4], decoded[-4:]
    expected = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    if checksum != expected:
        raise ValueError("invalid Base58Check checksum")
    return payload


def address_to_script_hash(address: str, *, address_version: int = DEFAULT_ADDRESS_VERSION) -> bytes:
    payload = base58check_decode(address)
    if len(payload) != 21:
        raise ValueError(
            f"invalid Neo address payload length: expected 21 bytes, got {len(payload)}"
        )
    version = payload[0]
    if version != address_version:
        raise ValueError(
            f"invalid Neo address version: expected 0x{address_version:02x}, got 0x{version:02x}"
        )
    return payload[1:]


def gas_account_storage_key(address: str, *, address_version: int = DEFAULT_ADDRESS_VERSION) -> str:
    script_hash = address_to_script_hash(address, address_version=address_version)
    key = bytes([NEP17_ACCOUNT_PREFIX]) + script_hash
    return base64.b64encode(key).decode("ascii")


def read_var_int(data: bytes, offset: int) -> tuple[int, int]:
    if offset >= len(data):
        raise ValueError("unexpected end of data while reading varint")
    first = data[offset]
    offset += 1
    if first < 0xFD:
        return first, offset
    if first == 0xFD:
        size = 2
    elif first == 0xFE:
        size = 4
    else:
        size = 8
    if offset + size > len(data):
        raise ValueError("unexpected end of data while reading extended varint")
    return int.from_bytes(data[offset : offset + size], "little"), offset + size


def decode_storage_integer(value_base64: str) -> int:
    value = base64.b64decode(value_base64)
    return int.from_bytes(value, "little", signed=True)


def decode_stack_integer(data: bytes, offset: int) -> tuple[int, int]:
    if offset >= len(data) or data[offset] != STACK_ITEM_INTEGER:
        actual = "EOF" if offset >= len(data) else f"0x{data[offset]:02x}"
        raise ValueError(f"expected StackItem Integer 0x21, got {actual}")
    offset += 1
    length, offset = read_var_int(data, offset)
    if offset + length > len(data):
        raise ValueError("unexpected end of data while reading StackItem Integer bytes")
    value = int.from_bytes(data[offset : offset + length], "little", signed=True)
    return value, offset + length


def decode_nep17_account_balance(value_base64: str | None) -> int:
    if not value_base64:
        return 0
    data = base64.b64decode(value_base64)
    if not data:
        return 0
    if data[0] != STACK_ITEM_STRUCT:
        raise ValueError(f"expected NEP-17 account Struct 0x41, got 0x{data[0]:02x}")
    count, offset = read_var_int(data, 1)
    if count < 1:
        raise ValueError("NEP-17 account Struct has no balance field")
    balance, _ = decode_stack_integer(data, offset)
    return balance


def rpc_call(url: str, method: str, params: list, timeout: float = 20.0) -> Any:
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    ).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=payload,
        headers={
            "Content-Type": "application/json",
            "Accept-Encoding": "identity",
            "User-Agent": "neo-rs-storage-validator/1.0",
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    data = json.loads(raw.decode("utf-8"))
    if data.get("error"):
        raise RuntimeError(f"{method} returned error from {url}: {data['error']}")
    return data["result"]
