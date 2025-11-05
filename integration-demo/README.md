# integration-demo

A small Tokio binary that exercises the standalone Neo N3 modules bundled in
this repository:

1. Seeds the in-memory store (`neo-store`) with header and block records.
2. Signs/verifies a payload using `neo-crypto` (P-256 ECDSA).
3. Drives the `neo-p2p` handshake helpers and broadcasts inventory, block, and
   address messages through the shared codec.
4. Demonstrates the `NeoEncode`/`NeoDecode` derive macros on a custom struct.

## Running

```bash
cargo run --manifest-path integration-demo/Cargo.toml
```

Available flags:

```bash
cargo run --manifest-path integration-demo/Cargo.toml -- --skip-store --skip-crypto
```

- `--skip-store` – bypasses the memory-store persistence step.
- `--skip-crypto` – skips signing/verifying the demo payload.
- `--skip-handshake` – skips the handshake and inventory simulation.

## Testing

A smoke test covers the full flow:

```bash
cargo test --manifest-path integration-demo/Cargo.toml
```

The binary depends on `neo-base`’s optional `derive` feature to keep the
serialization derived; disable it in your own crate if you want to implement
the traits manually.
