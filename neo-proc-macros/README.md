# neo-proc-macros

Derive macros that implement the `NeoEncode` and `NeoDecode` traits provided by
`neo-base`. They keep structs/enums declarative by auto-emitting the binary
codec logic that matches the Neo C# implementation.

## Example

```rust
use neo_base::{NeoDecode, NeoEncode, NeoDecodeDerive, NeoEncodeDerive, SliceReader};

#[derive(NeoEncodeDerive, NeoDecodeDerive)]
struct Header {
    version: u32,
    prev_hash: Hash256,
}

let header = Header { version: 0, prev_hash: Hash256::ZERO };
let mut buf = Vec::new();
header.neo_encode(&mut buf);
let mut reader = SliceReader::new(&buf);
let decoded = Header::neo_decode(&mut reader)?;
```

The derive supports:

- Named structs, tuple structs, and unit structs.
- Enums (each variant is tagged with a varint index in declaration order).

## Limitations

- Unions are not supported.
- The order of fields in structs/enums is significant; changes alter the wire
  format.

## Testing

```bash
cargo test --manifest-path neo-proc-macros/Cargo.toml
```
