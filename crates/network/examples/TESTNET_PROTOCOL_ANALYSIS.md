# TestNet Protocol Analysis

## Summary

Through debugging raw socket connections to Neo TestNet (34.133.235.69:20333), we discovered that TestNet uses a different protocol format than expected.

## Key Findings

### 1. Response Format
When sending a standard Neo protocol version message with TestNet magic bytes (0x56753345), the peer responds with:
```
00 00 25 4e 33 54 35 00 00 00 00 98 b7 89 68 9c
5a 52 1c 0b 2f 4e 65 6f 3a 33 2e 38 2e 32 2f 02
10 f4 cd 7c 00 01 6d 4f
```

### 2. Protocol Structure
The response appears to be a version message with this structure:
- Bytes 0-3: Version (0x00000000)
- Bytes 4-7: Network ID "N3T5" (0x4e335435)
- Bytes 8-11: Unknown field (0x00000000)
- Bytes 12-15: Timestamp
- Bytes 16-19: Nonce
- Byte 20: User agent length (11)
- Bytes 21-31: User agent "/Neo:3.8.2/"
- Remaining bytes: Additional protocol data

### 3. Key Differences
1. **No message envelope**: The response doesn't use the standard Neo message format with magic+command+length+checksum+payload
2. **Direct payload**: The peer sends the version payload directly without the message wrapper
3. **Network identifier**: Uses "N3T5" as a network identifier instead of relying solely on magic bytes

### 4. Implications
The TestNet appears to use a simplified protocol where:
- Messages are sent as raw payloads without the standard Neo message envelope
- The network is identified by the "N3T5" string in the payload
- This may be a newer protocol version or a TestNet-specific variation

## Debug Tools Created

1. **debug_testnet_standalone.rs**: Basic raw socket debugger
2. **debug_testnet_v2.rs**: Enhanced debugger with pattern analysis
3. **debug_testnet_proper.rs**: Full protocol debugger with message parsing
4. **analyze_testnet_response.rs**: Response analysis tool

## Next Steps

To properly connect to TestNet, the Neo-rs implementation needs to:
1. Support this alternative protocol format
2. Send/receive messages without the standard envelope when connecting to TestNet
3. Handle the "N3T5" network identifier
4. Adapt message parsing to handle direct payloads