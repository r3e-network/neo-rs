#!/usr/bin/env python3
"""Compare GAS token storage between local neo-rs and C# reference at a specific block height."""

import requests
import json
import base64
import sys

# Configuration
LOCAL_RPC = "http://127.0.0.1:20332"
REFERENCE_RPC = "http://seed1.neo.org:10332"
GAS_HASH = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
GAS_CONTRACT_ID = -6  # i32

def rpc(url, method, params):
    """Send JSON-RPC request."""
    r = requests.post(url, json={"jsonrpc": "2.0", "method": method, "params": params, "id": 1}, timeout=30)
    data = r.json()
    if "error" in data:
        raise Exception(f"RPC error: {data['error']}")
    return data.get("result")

def get_ref_state_root(height):
    """Get state root hash from reference at given height."""
    result = rpc(REFERENCE_RPC, "getstateroot", [height])
    return result["roothash"]

def find_ref_states(root_hash, prefix_b64, start_b64="", count=100):
    """Find storage entries on reference node."""
    results = []
    while True:
        batch = rpc(REFERENCE_RPC, "findstates", [root_hash, GAS_HASH, prefix_b64, start_b64, count])
        if not batch or "results" not in batch:
            break
        for entry in batch["results"]:
            results.append((entry["key"], entry["value"]))
        if batch.get("truncated", False) and batch["results"]:
            start_b64 = batch["results"][-1]["key"]
        else:
            break
    return results

def decode_neo_integer(data, offset):
    """Decode a Neo BinaryFormatWriter integer from data at offset.
    Returns (value, new_offset)."""
    # Type byte 0x21 = Integer
    if data[offset] != 0x21:
        raise ValueError(f"Expected Integer type 0x21, got 0x{data[offset]:02x}")
    offset += 1
    # Length byte (variable-length int, but for small values it's just 1 byte)
    length = data[offset]
    offset += 1
    if length == 0:
        return 0, offset
    int_bytes = data[offset:offset+length]
    value = int.from_bytes(int_bytes, byteorder='little', signed=True)
    return value, offset + length

def decode_gas_account_state(raw_bytes):
    """Decode a GAS AccountState from serialized StackItem bytes.

    Format (BinaryFormatWriter):
      0x41 = Struct type
      count (varint, usually 0x01 for GAS AccountState)
      0x21 = Integer type
      len  = byte length of integer
      bytes = little-endian signed integer (the balance)
    """
    if len(raw_bytes) < 3:
        return None

    offset = 0
    # Struct type
    if raw_bytes[offset] != 0x41:
        return None
    offset += 1

    # Count of struct fields
    count = raw_bytes[offset]
    offset += 1

    if count < 1:
        return None

    # First field: balance (Integer)
    balance, offset = decode_neo_integer(raw_bytes, offset)
    return balance


def main():
    height = int(sys.argv[1]) if len(sys.argv) > 1 else 295097

    print(f"Comparing GAS storage at block {height}")
    print(f"=" * 70)

    # Get reference state root
    root = get_ref_state_root(height)
    print(f"Reference state root: {root}")

    # PREFIX_ACCOUNT = 20 (0x14)
    prefix = base64.b64encode(bytes([0x14])).decode()

    print(f"\nFetching ALL GAS account balances from reference...")
    ref_entries = find_ref_states(root, prefix)
    print(f"Found {len(ref_entries)} account entries")

    # Our target account: 0x7903c54cf33cfe191b0dbf357e0a37c9596fa6f4
    # In Neo, UInt160 in storage keys is stored in little-endian (raw byte order).
    # The "0x" prefix version is big-endian (display order).
    # So 0x7903c54c... means the raw bytes are f4a66f59c9370a7e35bf0d1b19fe3cf34cc50379
    target_hash_be_display = "7903c54cf33cfe191b0dbf357e0a37c9596fa6f4"
    target_hash_le_raw = "f4a66f59c9370a7e35bf0d1b19fe3cf34cc50379"

    target_key = bytes([0x14]) + bytes.fromhex(target_hash_le_raw)

    # Show first 5 entries for format inspection
    print(f"\nFirst 5 entries (decoded):")
    for i, (key_b64, value_b64) in enumerate(ref_entries[:5]):
        key = base64.b64decode(key_b64)
        value = base64.b64decode(value_b64)
        balance = decode_gas_account_state(value)
        acct_le = key[1:].hex()
        acct_be = bytes(reversed(key[1:])).hex()
        gas = balance / 1e8 if balance else 0
        print(f"  [{i}] 0x{acct_be}: {balance:,} ({gas:.8f} GAS)  [hex: {value.hex()}]")

    # Search for target account
    print(f"\nSearching for target account 0x{target_hash_be_display}...")
    print(f"  (raw LE key: 14{target_hash_le_raw})")

    found = False
    for key_b64, value_b64 in ref_entries:
        key = base64.b64decode(key_b64)
        if key == target_key:
            value = base64.b64decode(value_b64)
            balance = decode_gas_account_state(value)
            gas = balance / 1e8 if balance else 0
            print(f"\n  FOUND!")
            print(f"  Raw hex: {value.hex()}")
            print(f"  Balance (smallest unit): {balance:,}")
            print(f"  Balance (GAS): {gas:.8f}")
            print(f"  Rust node balance (smallest unit): 6,207,581")
            print(f"  Rust node balance (GAS): {6207581 / 1e8:.8f}")
            if balance:
                diff = balance - 6207581
                print(f"  Difference: {diff:,} ({diff / 1e8:.8f} GAS)")
            found = True
            break

    if not found:
        print(f"  NOT FOUND at block {height}")
        print(f"\n  Trying direct getstate...")
        key_b64 = base64.b64encode(target_key).decode()
        try:
            val = rpc(REFERENCE_RPC, "getstate", [root, GAS_HASH, key_b64])
            if val:
                value = base64.b64decode(val)
                balance = decode_gas_account_state(value)
                gas = balance / 1e8 if balance else 0
                print(f"  Direct getstate found!")
                print(f"  Raw hex: {value.hex()}")
                print(f"  Balance (smallest unit): {balance:,}")
                print(f"  Balance (GAS): {gas:.8f}")
            else:
                print(f"  Direct getstate returned empty")
        except Exception as e:
            print(f"  Direct getstate: {e}")

    # Decode all accounts and sort by balance
    print(f"\n{'=' * 70}")
    print(f"Top 20 accounts by balance:")
    accounts = []
    for key_b64, value_b64 in ref_entries:
        key = base64.b64decode(key_b64)
        value = base64.b64decode(value_b64)
        if len(key) == 21 and key[0] == 0x14:
            balance = decode_gas_account_state(value)
            if balance is not None:
                acct_le = key[1:].hex()
                acct_be = bytes(reversed(key[1:])).hex()
                accounts.append((acct_be, balance))

    accounts.sort(key=lambda x: -x[1])
    for acc, bal in accounts[:20]:
        gas = bal / 1e8
        marker = " <<<" if acc == target_hash_be_display else ""
        print(f"  0x{acc}: {bal:>20,} ({gas:>16.8f} GAS){marker}")

    # Total supply
    print(f"\nGAS Total Supply:")
    ts_prefix = base64.b64encode(bytes([0x0b])).decode()
    try:
        ts_entries = find_ref_states(root, ts_prefix)
        if ts_entries:
            for key_b64, value_b64 in ts_entries:
                value = base64.b64decode(value_b64)
                # Total supply is stored as raw BigInteger, not StackItem
                ts = int.from_bytes(value, byteorder='little', signed=True)
                print(f"  Raw: {ts:,} ({ts / 1e8:.8f} GAS)")
    except Exception as e:
        print(f"  Error: {e}")

    # Sum all balances for consistency check
    total_balances = sum(b for _, b in accounts)
    print(f"\n  Sum of all balances: {total_balances:,} ({total_balances / 1e8:.8f} GAS)")
    print(f"  Accounts: {len(accounts)}")

if __name__ == "__main__":
    main()
