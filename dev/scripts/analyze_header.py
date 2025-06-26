#!/usr/bin/env python3
# Parse the header bytes
header = [0x00, 0x00, 0x25, 0x4e, 0x33, 0x54, 0x35, 0x00]
print('Header bytes:', ' '.join(f'{b:02x}' for b in header))

# Extract magic (first 4 bytes)
magic = (header[3] << 24) | (header[2] << 16) | (header[1] << 8) | header[0]
print(f'Magic (as u32 LE): 0x{magic:08x} ({magic})')

# Try as BE
magic_be = (header[0] << 24) | (header[1] << 16) | (header[2] << 8) | header[3]
print(f'Magic (as u32 BE): 0x{magic_be:08x} ({magic_be})')

# Check if these could be ASCII
print('\nAs ASCII:', ''.join(chr(b) if 32 <= b < 127 else f'\\x{b:02x}' for b in header))

# Neo TestNet magic should be 0x74746E41
testnet_magic = 0x74746E41
print(f'\nExpected TestNet magic: 0x{testnet_magic:08x}')
print('Expected bytes (LE):', ' '.join(f'{b:02x}' for b in testnet_magic.to_bytes(4, 'little')))

# Wait, the received bytes seem odd. Let's check if we're receiving something else
print('\nAnalyzing full 8 bytes as potential version number + other data:')
print('Bytes 0-3:', ' '.join(f'{b:02x}' for b in header[:4]))
print('Bytes 4-7:', ' '.join(f'{b:02x}' for b in header[4:]))

# Check if this could be a version message without proper header
# The bytes look like they might be part of a different structure
print('\nChecking if this could be raw data without header:')
# 0x00 0x00 might be version (0)
# 0x25 0x4e might be services (0x4e25 = 20005)
# Let's see what 0x4e25 is
services = (header[3] << 8) | header[2]
print(f'If bytes 2-3 are services: 0x{services:04x} ({services})')

# The pattern suggests we might not be reading the actual message header
# but some other data structure