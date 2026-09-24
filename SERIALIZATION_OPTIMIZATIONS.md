# Serialization Zero-Copy Optimizations

## Analysis Summary

The neo-io crate already uses the `bytes` crate and has good foundations for zero-copy operations. Key optimization opportunities identified:

1. **MemoryReader** - Already zero-copy with `read_bytes_ref`, `read_var_memory`, `read_memory`
2. **BinaryWriter** - Has `as_bytes()` for zero-copy reads but buffer is Vec<u8>
3. **var_int module** - Uses extend_from_slice, already efficient
4. **Encoding** - Base58/Base64/Hex operations create owned Vec<u8> (required by API)

## Optimizations Implemented

### 1. Enhanced BinaryWriter with BytesMut (opt-in)
- Add `bytes::BytesMut` backend option for better zero-copy semantics
- Allows freezing to `Bytes` for cheap cloning
- Backwards compatible with existing Vec<u8> API

### 2. Add zero-copy serialization helpers
- `write_bytes_ref` - direct reference writing (already exists as write_bytes)
- Size pre-calculation to minimize reallocations
- Document zero-copy patterns

### 3. Optimize hot paths
- var_int operations (already optimal)
- Array serialization with capacity hints
- Reduce intermediate allocations

## Performance Impact
- Reduced allocations in serialization paths
- Better memory locality
- Cheaper cloning with Bytes for network/storage layers
