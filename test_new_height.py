#!/usr/bin/env python3

# Fresh TestNet payload from our latest run:
payload = [0x00, 0x00, 0x25, 0x4e, 0x33, 0x54, 0x35, 0x00, 0x00, 0x00, 0x00, 0x3d, 0xd1, 0x89, 0x68, 0x38, 0x9b, 0x8f, 0x1b, 0x0b, 0x2f, 0x4e, 0x65, 0x6f, 0x3a, 0x33, 0x2e, 0x38, 0x2e, 0x32, 0x2f, 0x02, 0x10, 0x1f, 0xd6, 0x7c, 0x00, 0x01, 0x6d, 0x4f]

print("Current extraction methods:")
print()

# Old method (bytes 34-37)
old_height = int.from_bytes([payload[34], payload[35], payload[36], payload[37]], byteorder='little')
print(f"OLD (bytes 34-37): {payload[34]:02x} {payload[35]:02x} {payload[36]:02x} {payload[37]:02x} = {old_height:,} blocks")

# New method (bytes 33-36)  
new_height = int.from_bytes([payload[33], payload[34], payload[35], payload[36]], byteorder='little')
print(f"NEW (bytes 33-36): {payload[33]:02x} {payload[34]:02x} {payload[35]:02x} {payload[36]:02x} = {new_height:,} blocks")

print()
print("Analysis:")
print(f"Old height ({old_height:,}) - Does this look like TestNet? {'❌ NO' if old_height > 10_000_000 else '✅ YES'}")
print(f"New height ({new_height:,}) - Does this look like TestNet? {'✅ YES' if 7_000_000 <= new_height <= 9_000_000 else '❌ NO'}")