# Neo N3 Rust Experimental Modules

This repository hosts a set of standalone crates that implement fundamental
building blocks of a Neo N3 node in idiomatic Rust. They live outside the
primary workspace while the architecture settles, allowing each module to
iterate quickly.

| Crate | Responsibility |
| ----- | -------------- |
| [`neo-base/`](neo-base) | Deterministic `NeoEncode`/`NeoDecode` binary codec, typed `Bytes`/`Hash160`/`Hash256`, Merkle tooling, optional derive macros, `no_std`-friendly core. |
| [`neo-crypto/`](neo-crypto) | Secret-key wrapper with zeroize + constant-time equality, P-256 ECDSA sign/verify, AES-256-ECB, HMAC-SHA256, scrypt key stretching. |
| [`neo-store/`](neo-store) | `Store` trait, typed column families (`Headers`, `Blocks`), in-memory backend with batching & snapshots, JSON fixtures for seeding data. |
| [`neo-p2p/`](neo-p2p) | Wire protocol primitives: version/handshake, ping/pong, inventory/getdata, block/tx payloads, Tokio codec, peer state machine. |
| [`neo-proc-macros/`](neo-proc-macros) | `NeoEncode`/`NeoDecode` derive macros to remove manual serialization code. |

If you are looking for the quickest way to explore how these crates work
together, start with [`docs/QUICKSTART.md`](docs/QUICKSTART.md).

Longer-term tasks are tracked in [`docs/ROADMAP.md`](docs/ROADMAP.md).

The `integration-demo/` crate demonstrates an end-to-end flow:

```bash
cargo run --manifest-path integration-demo/Cargo.toml
cargo test --manifest-path integration-demo/Cargo.toml
```

**Demo flow**

1. Persist a header/block pair via the in-memory store (`StoreExt` helpers).
2. Sign and verify a message with `neo-crypto` using the shared codec types.
3. Walk through the P2P handshake and broadcast inventory + block payloads via
   the shared `NeoMessageCodec`.
4. Round-trip a derived struct using the `NeoEncode`/`NeoDecode` macros.

Flags such as `--skip-store`, `--skip-crypto`, and `--skip-handshake` (passed
after `--`) let you focus on individual phases without running the full demo.

Each crate has its own `Cargo.toml`, so you can run tests individually, for
example:

```bash
cargo test --manifest-path neo-base/Cargo.toml --features derive
cargo test --manifest-path neo-crypto/Cargo.toml
cargo test --manifest-path neo-store/Cargo.toml
cargo test --manifest-path neo-p2p/Cargo.toml
cargo test --manifest-path neo-proc-macros/Cargo.toml
```

Or run them all via the helper script:

```bash
./scripts/test_all.sh
```

See [`scripts/README.md`](scripts/README.md) for the list of available utilities.

> **Note**: These crates are experimental and evolve rapidly; breaking changes
> should be expected until they are promoted into the primary workspace.
