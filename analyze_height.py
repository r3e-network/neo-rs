#!/usr/bin/env python3

# Sample TestNet version payload from our logs:
# [00, 00, 25, 4e, 33, 54, 35, 00, 00, 00, 00, 94, cd, 89, 68, 9c, 5a, 52, 1c, 0b, 2f, 4e, 65, 6f, 3a, 33, 2e, 38, 2e, 32, 2f, 02, 10, f1, d4, 7c, 00, 01, 6d, 4f]

payload = [0x00, 0x00, 0x25, 0x4e, 0x33, 0x54, 0x35, 0x00, 0x00, 0x00, 0x00, 0x94, 0xcd, 0x89, 0x68, 0x9c, 0x5a, 0x52, 0x1c, 0x0b, 0x2f, 0x4e, 0x65, 0x6f, 0x3a, 0x33, 0x2e, 0x38, 0x2e, 0x32, 0x2f, 0x02, 0x10, 0xf1, 0xd4, 0x7c, 0x00, 0x01, 0x6d, 0x4f]

print("TestNet version payload analysis:")
print("Payload length:", len(payload))
print("Raw payload:", [hex(b) for b in payload])
print()

# Expected TestNet height is around 8 million blocks
expected_height_range = (7_000_000, 9_000_000)

print("Searching for realistic TestNet height (7M-9M blocks):")
print()

# Check all possible 4-byte little-endian sequences
for start_byte in range(len(payload) - 3):
    # Extract 4 bytes and convert from little-endian
    height_bytes = payload[start_byte:start_byte + 4]
    height = int.from_bytes(height_bytes, byteorder='little')
    
    # Also try big-endian
    height_be = int.from_bytes(height_bytes, byteorder='big')
    
    status_le = "✅" if expected_height_range[0] <= height <= expected_height_range[1] else "  "
    status_be = "✅" if expected_height_range[0] <= height_be <= expected_height_range[1] else "  "
    
    print(f"Bytes {start_byte:2d}-{start_byte+3:2d}: {height_bytes[0]:02x} {height_bytes[1]:02x} {height_bytes[2]:02x} {height_bytes[3]:02x} = {height:>10,} (LE) {status_le} | {height_be:>10,} (BE) {status_be}")

print()
print("Legend: LE = Little Endian, BE = Big Endian")
print("✅ = Within expected TestNet height range (7M-9M)")