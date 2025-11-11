# neo-p2p

The `neo-p2p` crate provides the serialization logic and small utilities around
the Neo N3 peer-to-peer protocol. It does **not** open sockets; instead, it
focuses on deterministic encoding/decoding and the state transitions needed to
drive a handshake from higher-level networking code.

## Features

- Message types covering the essential wire commands:
  - `version` / `verack`
  - `ping` / `pong`
  - `getaddr` / `addr`
  - `inv` / `getdata`
  - `block` / `tx`
- `NeoMessageCodec` (Tokio codec) for Neo wire frames (network magic,
  ASCII command padded to 12 bytes, payload length, checksum, and the inner
  message with compression support). Configure it via `with_network_magic`.
  `neo-base` binary encoding.
- `HandshakeMachine` and `Peer` helpers to manage the version/verack exchange
  and track readiness.

## Testing

Unit tests exercise round-trip serialization for every message type as well as
the handshake paths:

```bash
cargo test --manifest-path neo-p2p/Cargo.toml
```

The integration demo (`integration-demo/`) uses these APIs to simulate a small
handshake and inventory broadcast.

## Examples

An async handshake helper showcases how to connect to a Neo node, drive the
version/verack exchange, request peer addresses, send a ping, optionally ask
for headers, and (with `--request-data`) issue `getdata` when peers announce
inventory:

```bash
cargo run --manifest-path neo-p2p/Cargo.toml --example handshake -- \
  --target seed1.ngd.network:10333 --network 860833102 \
  --request-headers --start-index 1000 \
  --request-data --store-blocks --store-txs --dump-dir ./payloads
```
