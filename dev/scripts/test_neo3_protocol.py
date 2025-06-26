#!/usr/bin/env python3
import socket
import struct
import hashlib
import time

def create_neo3_version_message():
    """Create a Neo 3 version message with the new format."""
    # Neo 3 message format:
    # - Flags (1 byte)
    # - Command (1 byte) 
    # - Payload length (variable length encoding)
    # - Payload
    
    # First create the version payload
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
    
    # Now create the message with Neo 3 format
    message = bytearray()
    
    # Flags (1 byte) - 0 for no compression
    message.append(0)
    
    # Command (1 byte) - 0x00 for Version
    message.append(0x00)
    
    # Payload length (variable length encoding)
    payload_len = len(payload)
    if payload_len < 0xFD:
        message.append(payload_len)
    elif payload_len <= 0xFFFF:
        message.append(0xFD)
        message.extend(struct.pack('<H', payload_len))
    elif payload_len <= 0xFFFFFFFF:
        message.append(0xFE)
        message.extend(struct.pack('<I', payload_len))
    else:
        message.append(0xFF)
        message.extend(struct.pack('<Q', payload_len))
    
    # Payload
    message.extend(payload)
    
    return bytes(message)

def parse_neo3_message(data):
    """Parse a Neo 3 message."""
    if len(data) < 3:
        print("Message too short")
        return
    
    offset = 0
    
    # Flags
    flags = data[offset]
    print(f"Flags: 0x{flags:02x} (Compressed: {'Yes' if flags & 1 else 'No'})")
    offset += 1
    
    # Command
    command = data[offset]
    print(f"Command: 0x{command:02x}")
    offset += 1
    
    # Payload length (variable encoding)
    length_byte = data[offset]
    offset += 1
    
    if length_byte < 0xFD:
        payload_len = length_byte
    elif length_byte == 0xFD:
        payload_len = struct.unpack('<H', data[offset:offset+2])[0]
        offset += 2
    elif length_byte == 0xFE:
        payload_len = struct.unpack('<I', data[offset:offset+4])[0]
        offset += 4
    elif length_byte == 0xFF:
        payload_len = struct.unpack('<Q', data[offset:offset+8])[0]
        offset += 8
    
    print(f"Payload length: {payload_len} bytes")
    
    # Check if we have enough data
    if len(data) < offset + payload_len:
        print(f"Incomplete payload: expected {payload_len} bytes, have {len(data) - offset}")
        return
    
    # If it's a version message, parse the payload
    if command == 0x00:
        print("\nParsing Version payload:")
        parse_version_payload(data[offset:offset+payload_len])

def parse_version_payload(payload):
    """Parse version payload."""
    offset = 0
    
    # Version
    version = struct.unpack('<I', payload[offset:offset+4])[0]
    print(f"  Version: {version}")
    offset += 4
    
    # Services
    services = struct.unpack('<Q', payload[offset:offset+8])[0]
    print(f"  Services: {services}")
    offset += 8
    
    # Timestamp
    timestamp = struct.unpack('<Q', payload[offset:offset+8])[0]
    print(f"  Timestamp: {timestamp}")
    offset += 8
    
    # Port
    port = struct.unpack('<H', payload[offset:offset+2])[0]
    print(f"  Port: {port}")
    offset += 2
    
    # Nonce
    nonce = struct.unpack('<I', payload[offset:offset+4])[0]
    print(f"  Nonce: {nonce}")
    offset += 4
    
    # User agent
    ua_len = payload[offset]
    offset += 1
    user_agent = payload[offset:offset+ua_len].decode('ascii', errors='ignore')
    print(f"  User Agent: '{user_agent}'")
    offset += ua_len
    
    # Start height
    start_height = struct.unpack('<I', payload[offset:offset+4])[0]
    print(f"  Start Height: {start_height}")
    offset += 4
    
    # Relay
    relay = payload[offset]
    print(f"  Relay: {relay} ({'True' if relay else 'False'})")

def hex_dump(data):
    """Print hex dump of data."""
    for i in range(0, len(data), 16):
        hex_part = ' '.join(f'{b:02x}' for b in data[i:i+16])
        ascii_part = ''.join(chr(b) if 32 <= b < 127 else '.' for b in data[i:i+16])
        print(f"{i:04x}: {hex_part:<48}  |{ascii_part}|")

def main():
    print("=== Neo 3 P2P Protocol Test ===\n")
    
    # Test connection to Neo TestNet node
    addr = ("34.133.235.69", 20333)
    print(f"Connecting to {addr[0]}:{addr[1]}...")
    
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(10)
        sock.connect(addr)
        print("✓ Connected successfully!\n")
        
        # Create and send Neo 3 version message
        version_msg = create_neo3_version_message()
        print(f"Neo 3 Version message ({len(version_msg)} bytes):")
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
            
            print("\nParsing as Neo 3 message:")
            parse_neo3_message(response)
        else:
            print("No response received")
        
        sock.close()
        
    except Exception as e:
        print(f"✗ Error: {e}")

if __name__ == "__main__":
    main()