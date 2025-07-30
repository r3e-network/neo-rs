#!/usr/bin/env python3
import struct

# The response appears to be a raw version payload without header
response = bytes([0x00, 0x00, 0x25, 0x4e, 0x33, 0x54, 0x35, 0x00, 
                  0x00, 0x00, 0x00, 0x39, 0x97, 0x5c, 0x68, 0x4c,
                  0x05, 0x7c, 0x6a, 0x0b, 0x2f, 0x4e, 0x65, 0x6f, 
                  0x3a, 0x33, 0x2e, 0x38, 0x2e, 0x32, 0x2f, 0x02,
                  0x10, 0x87, 0x6d, 0x6e, 0x00, 0x01, 0x6d, 0x4f])

print("=== Analyzing as Raw Version Payload ===\n")

# We know user agent is at offset 20 with length 11
# Let's work backwards from there

# User agent at 20, length byte at 19
# So nonce should be at 15-18 (4 bytes)
# Port should be at 13-14 (2 bytes)
# Timestamp should be at 5-12 (8 bytes)
# Services should be at/* Implementation needed */ wait, this doesn't align

# Let me try a different approach
# What if the version uses different field sizes?

print("Attempt 1: Standard field sizes from user agent position")
ua_start = 20
ua_len = 11

# Working backwards:
# - User agent length at 19
# - Nonce at 15-18
nonce = struct.unpack('<I', response[15:19])[0]
print(f"Nonce: {nonce} (0x{nonce:08x})")

# - Port at 13-14
port = struct.unpack('<H', response[13:15])[0]
print(f"Port: {port}")

# Now the issue - we have 13 bytes before port
# Standard would be version(4) + services(8) = 12
# But we have 13/* Implementation needed */

print("\nAttempt 2: What if timestamp is 4 bytes instead of 8?")
# Version at 0-3
version = struct.unpack('<I', response[0:4])[0]
print(f"Version: {version} (0x{version:08x})")

# Hmm, that gives us 0x4e250000 which is huge

print("\nAttempt 3: Checking if there's padding or different structure")
# Let's look at the pattern more carefully
print("\nByte-by-byte analysis:")
for i in range(min(20, len(response))):
    print(f"Offset {i:2d}: 0x{response[i]:02x} ({response[i]:3d}) {chr(response[i]) if 32 <= response[i] < 127 else '.'}")

print("\n--- Most likely interpretation ---")
# Given that we see the user agent clearly at the right position,
# and the peer is Neo:3.8.2, this might be using a different
# version message format or the response is not what we expect

# Check if this could be a different message type
print("\nChecking if bytes 4-16 could be a command:")
possible_command = response[4:16]
# Remove null padding
cmd_str = possible_command.rstrip(b'\x00')
print(f"As string: '{cmd_str}' (hex: {cmd_str.hex()})")

# The fact that we're getting raw data suggests either:
# 1. The peer expects a different initial message
# 2. The peer is sending version payload without header
# 3. We're not reading at the right boundary

print("\n--- Conclusion ---")
print("The response appears to be a raw version payload without the standard Neo message header.")
print("This suggests the Neo node might be using a different handshake protocol than expected.")
print("The peer is running Neo:3.8.2, which we can see clearly in the response.")