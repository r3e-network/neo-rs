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
- `NeoMessageCodec` (Tokio codec) for length-prefixed framing using the shared
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
