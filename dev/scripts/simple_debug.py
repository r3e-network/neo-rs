#!/usr/bin/env python3
import socket
import struct
import hashlib
import time

def create_version_message():
    """Create a Neo version message."""
    # Create payload first
    payload = bytearray()
    
    # Version (4 bytes)
    payload.extend(struct.pack('<I', 0))
    
    # Services (8 bytes)
    payload.extend(struct.pack('<Q', 1))
    
    # Timestamp (8 bytes)
    payload.extend(struct.pack('<Q', int(time.time())))
    
    # Port (2 bytes)
    payload.extend(struct.pack('<H', 20333))
    
    # Nonce (4 bytes)
    payload.extend(struct.pack('<I', 12345))
    
    # User agent (var string)
    user_agent = b"/Neo-Rust:0.1.0/"
    payload.append(len(user_agent))
    payload.extend(user_agent)
    
    # Start height (4 bytes)
    payload.extend(struct.pack('<I', 0))
    
    # Relay (1 byte)
    payload.append(1)
    
    # Create header
    header = bytearray()
    
    # Magic: TestNet = 0x74746E41
    header.extend(struct.pack('<I', 0x74746E41))
    
    # Command: "version" (12 bytes, zero-padded)
    command = b"version" + b'\x00' * (12 - len("version"))
    header.extend(command)
    
    # Length
    header.extend(struct.pack('<I', len(payload)))
    
    # Checksum (double SHA256 of payload)
    hash1 = hashlib.sha256(bytes(payload)).digest()
    hash2 = hashlib.sha256(hash1).digest()
    header.extend(hash2[:4])
    
    return bytes(header + payload)

def parse_response(data):
    """Parse Neo message header."""
    if len(data) < 24:
        print(f"Response too short: {len(data)} bytes")
        return
    
    magic = struct.unpack('<I', data[0:4])[0]
    command = data[4:16].rstrip(b'\x00').decode('ascii', errors='ignore')
    length = struct.unpack('<I', data[16:20])[0]
    checksum = struct.unpack('<I', data[20:24])[0]
    
    print(f"  Magic: 0x{magic:08x}")
    print(f"  Command: '{command}'")
    print(f"  Payload length: {length} bytes")
    print(f"  Checksum: 0x{checksum:08x}")

def hex_dump(data):
    """Print hex dump of data."""
    for i in range(0, len(data), 16):
        hex_part = ' '.join(f'{b:02x}' for b in data[i:i+16])
        ascii_part = ''.join(chr(b) if 32 <= b < 127 else '.' for b in data[i:i+16])
        print(f"{i:04x}: {hex_part:<48}  |{ascii_part}|")

def main():
    print("=== Neo P2P Protocol Debug ===\n")
    
    # Test connection to Neo TestNet node
    addr = ("34.133.235.69", 20333)
    print(f"Connecting to {addr[0]}:{addr[1]}...")
    
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(10)
        sock.connect(addr)
        print("✓ Connected successfully!\n")
        
        # Create and send version message
        version_msg = create_version_message()
        print(f"Version message ({len(version_msg)} bytes):")
        hex_dump(version_msg)
        
        print("\nSending version message...")
        sock.send(version_msg)
        print("✓ Sent successfully!")
        
        # Read response
        print("\nReading response...")
        response = sock.recv(1024)
        
        if response:
            print(f"Received {len(response)} bytes:")
            hex_dump(response)
            
            print("\nParsing as Neo message header:")
            parse_response(response)
        else:
            print("No response received")
        
        sock.close()
        
    except Exception as e:
        print(f"✗ Error: {e}")

if __name__ == "__main__":
    main()