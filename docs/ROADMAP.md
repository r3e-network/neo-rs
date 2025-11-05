# Roadmap (Experimental Modules)

This roadmap tracks upcoming work for the standalone Neo N3 Rust crates before
they are merged into the primary workspace.

## neo-base
- [ ] Expand codec derives with field attributes (e.g. version gating, fixed
  length arrays).
- [ ] Add property-based tests for `NeoEncode`/`NeoDecode` round-trips.
- [ ] Provide `From`/`TryFrom` helpers for additional hash types (script hash,
  witness hash).

## neo-crypto
- [ ] Support message length checks in AES helpers (padding strategies).
- [ ] Add NEP-2 / NEP-6 conversion utilities.
- [ ] Benchmark signing/verification path with criterion.

## neo-p2p
- [ ] Implement block/tx compression flags once the storage format is ready.
- [ ] Introduce misbehavior scoring for handshake/heartbeat failures.
- [ ] Connect codec to a sample TCP transport (`tokio::net::TcpStream`).

## neo-store
- [ ] Provide a RocksDB backend with identical trait surface.
- [ ] Add iterators for range scans and prefix queries.
- [ ] Persist fixtures to disk for replay across tests.

## integration-demo
- [ ] Add CLI options to toggle which steps to run (store-only, handshake-only).
- [ ] Hook into `scripts/test_all.sh` for CI.
- [ ] Document expected output and extend tests to assert codec frames.
