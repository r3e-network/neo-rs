#!/usr/bin/env python3
# Analyze the response from Neo node
response = bytes([0x00, 0x00, 0x25, 0x4e, 0x33, 0x54, 0x35, 0x00, 
                  0x00, 0x00, 0x00, 0x39, 0x97, 0x5c, 0x68, 0x4c,
                  0x05, 0x7c, 0x6a, 0x0b, 0x2f, 0x4e, 0x65, 0x6f, 
                  0x3a, 0x33, 0x2e, 0x38, 0x2e, 0x32, 0x2f, 0x02,
                  0x10, 0x87, 0x6d, 0x6e, 0x00, 0x01, 0x6d, 0x4f])

print("=== Analyzing Neo Node Response ===\n")
print(f"Total bytes received: {len(response)}")
print("\nHex dump:")
for i in range(0, len(response), 16):
    hex_part = ' '.join(f'{b:02x}' for b in response[i:i+16])
    ascii_part = ''.join(chr(b) if 32 <= b < 127 else '.' for b in response[i:i+16])
    print(f"{i:04x}: {hex_part:<48}  |{ascii_part}|")

print("\n--- Parsing as Version Payload (not header) ---")
# What if this is the version payload directly without header?
import struct

offset = 0
try:
    # Version (4 bytes)
    version = struct.unpack('<I', response[offset:offset+4])[0]
    print(f"Version: {version}")
    offset += 4
    
    # Services (8 bytes)
    services = struct.unpack('<Q', response[offset:offset+8])[0]
    print(f"Services: {services}")
    offset += 8
    
    # Timestamp (8 bytes)
    timestamp = struct.unpack('<Q', response[offset:offset+8])[0]
    print(f"Timestamp: {timestamp}")
    offset += 8
    
    # Port (2 bytes)
    port = struct.unpack('<H', response[offset:offset+2])[0]
    print(f"Port: {port}")
    offset += 2
    
    # Nonce (4 bytes)
    nonce = struct.unpack('<I', response[offset:offset+4])[0]
    print(f"Nonce: {nonce}")
    offset += 4
    
    # User agent (var string)
    ua_len = response[offset]
    offset += 1
    user_agent = response[offset:offset+ua_len].decode('ascii')
    print(f"User Agent: '{user_agent}' (length: {ua_len})")
    offset += ua_len
    
    # Start height (4 bytes)
    start_height = struct.unpack('<I', response[offset:offset+4])[0]
    print(f"Start Height: {start_height}")
    offset += 4
    
    # Relay (1 byte)
    relay = response[offset]
    print(f"Relay: {relay} ({'True' if relay else 'False'})")
    
except Exception as e:
    print(f"\nError parsing: {e}")

print("\n--- Alternative interpretation ---")
# Maybe the peer is expecting something else first?
# Let's check if the pattern matches any known structure

# The bytes 0x00 0x00 could be version 0
# 0x25 0x4e could be part of something else
# The string /Neo:3.8.2/ is clearly visible

# Search for the user agent string
ua_start = response.find(b'/Neo:')
if ua_start >= 0:
    print(f"Found user agent at offset {ua_start}: {response[ua_start:ua_start+11]}")
    
    # Work backwards from there
    print("\nTrying to parse from user agent backwards"Implementation complete"")
    # The byte before /Neo: should be the length (11)
    if ua_start > 0:
        ua_len_byte = response[ua_start-1]
        print(f"User agent length byte: {ua_len_byte} (expected 11 for '/Neo:3.8.2/')")