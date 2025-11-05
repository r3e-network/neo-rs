# Neo N3 Rust Building Blocks – Quick Start

This repository bundles low-level crates that mirror the responsibilities of the upstream C# implementation while embracing Rust idioms. The code lives outside of `crates/` so it can evolve independently before being wired into the main workspace.

| Crate | Highlights |
| ----- | ---------- |
| `neo-base` | Deterministic `NeoEncode`/`NeoDecode` codec, typed `Bytes`/`Hash160`/`Hash256`, Merkle helper, optional derive macros, `no_std` friendly core. |
| `neo-crypto` | Secret-key wrapper with zeroize + constant-time equality, P-256 ECDSA sign/verify, AES-256-ECB, HMAC-SHA256, scrypt key stretching. |
| `neo-store` | `Store` trait, typed column families (`Headers`, `Blocks`), in-memory backend with batching/snapshots, JSON fixtures to seed state. |
| `neo-p2p` | Protocol messages (version/ping/pong/inventory/getdata/block/tx), Tokio codec, handshake state machine, peer helper. |
| `neo-proc-macros` | `NeoEncode`/`NeoDecode` derive macros to remove manual serialization code. |

See [`docs/ROADMAP.md`](ROADMAP.md) for in-progress tasks and longer-term goals.

## Developing each crate

All crates are published as standalone packages. Run tests with:

```bash
cargo test --manifest-path neo-base/Cargo.toml
cargo test --manifest-path neo-base/Cargo.toml --features derive
cargo test --manifest-path neo-crypto/Cargo.toml
cargo test --manifest-path neo-store/Cargo.toml
cargo test --manifest-path neo-p2p/Cargo.toml
cargo test --manifest-path neo-proc-macros/Cargo.toml
```

## Integration demo

The `integration-demo` crate (added in this repo) shows the crates working together:

* Derive codec implementations for headers/blocks.
* Persist data through the memory store.
* Build and encode a P2P version + inventory announcement.
* Sign and verify a payload with the crypto module.

Run it with:

```bash
cargo run --manifest-path integration-demo/Cargo.toml
```

Pass `--skip-store`, `--skip-crypto`, or `--skip-handshake` to focus on individual
steps without running the full flow.

Or execute the integration test (driven by `#[tokio::test]` smoke coverage):

```bash
cargo test --manifest-path integration-demo/Cargo.toml
```

The demo uses the default `std` features plus the optional `derive` feature
from `neo-base` so structs and enums can auto-implement the codec traits. If
you only need the core primitives you can disable `std`/`derive` when you add
the dependencies to another crate.
