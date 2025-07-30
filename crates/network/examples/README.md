# Network Examples

## debug_testnet.rs

A simple debugging tool for Neo TestNet connections that captures raw bytes without parsing assumptions.

### Usage

```bash
cargo run --example debug_testnet
```

### What it does

1. Connects to a TestNet peer at 34.133.235.69:20333
2. Sends a minimal version message with TestNet magic bytes (0x56753345)
3. Reads raw bytes from the socket and displays them in:
   - Hexadecimal format
   - ASCII (printable characters only)
4. Identifies magic byte boundaries in responses
5. Times out after 10 seconds of no data

### Purpose

This tool helps debug protocol issues by showing exactly what bytes are being sent by TestNet peers without any parsing logic that might hide or misinterpret data.

### Example Output

```
Connecting to Neo TestNet at 34.133.235.69:20333...
Connected! Sending version message...
Sending version message (85 bytes):
Hex: 45337556766572...

Listening for responses...
Received 24 bytes:
Hex: 453375567665726163...
ASCII (printable only): E3uVverack......
Found magic bytes at offset 0
```