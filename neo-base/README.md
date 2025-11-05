# neo-base

`neo-base` bundles the low-level primitives shared across the Neo N3 Rust
implementation. It focuses on deterministic serialization, typed byte/ hash
wrappers, and utilities that work in both `std` and `no_std` contexts.

## Highlights

- `NeoEncode` / `NeoDecode` traits with varint framing consistent with the C#
  implementation.
- `Bytes`, `Hash160`, `Hash256` wrappers with serde support and optional
  derive macros for “derive-first” structures.
- Merkle helper, hashing utilities (`sha256`, `double_sha256`, `hash160`).
- `derive` feature that re-exports the procedural macros from
  `neo-proc-macros`.

## Features

```toml
[dependencies]
neo-base = { path = "../neo-base", default-features = false }

[features]
std = ["neo-base/std"]
derive = ["neo-base/derive"]
```

Enable `std` if you need `Vec`, `serde_json` tests, or the `time` module.

## Testing

```bash
cargo test --manifest-path neo-base/Cargo.toml
cargo test --manifest-path neo-base/Cargo.toml --features derive
```
