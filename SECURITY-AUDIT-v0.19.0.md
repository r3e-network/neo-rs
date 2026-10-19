# Dependency Security Audit — v0.19.0

Date: 2026-10-19  
Database: RustSec advisory DB (2026-10-17 snapshot)  
Tool: `cargo audit` (536 dependencies)

## Status

`cargo audit` is **not clean** for v0.19.0. The release is a testnet /
private-network release candidate with the following documented exceptions.

Directly fixed in this release:

| Crate | Before | After |
|---|---|---|
| `lru` | 0.12.5 (unsound) | **0.18.5** |
| `rustls` (client stack) | 0.23.45 | **0.23.46** |

## Remaining advisories

### `h2 0.3.27` — RUSTSEC-2026-0258 (unbounded empty DATA frames)

- Pulled in by `hyper 0.14` / `warp 0.3`, which are used only by the RPC and
  telemetry HTTP servers.
- **Mitigation:** both server paths now call `hyper::Server::builder(...).http1_only(true)`,
  so HTTP/2 prior-knowledge or ALPN negotiation cannot reach the `h2` state
  machine. The patched `h2 0.4.19` is already present for the reqwest/hyper 1
  client stack.
- Follow-up: migrate the HTTP server from hyper 0.14/warp 0.3 to hyper 1 and
  remove `h2 0.3` from the graph.

### `rustls 0.21.12` / `rustls-webpki 0.101.7` — RPC TLS server

The RPC TLS listener still uses `rustls 0.21` + `tokio-rustls 0.24`.

Relevant advisories are client-side or optional:

- RUSTSEC-2026-0346: `Debug` of client session values — the server does not use
  a custom client session store.
- RUSTSEC-2026-0347: ECH rejection data — ECH is not enabled.
- RUSTSEC-2026-0348: Brotli certificate decompression — the `brotli` feature is
  not enabled.
- RUSTSEC-2026-0349: client key-share prediction — client-side only.
- rustls-webpki name-constraint advisories: only reachable when validating
  client certificates against configured trusted authorities. The RPC server
  pins native roots by SHA-1 thumbprint before adding them, and TLS with client
  authentication is opt-in via `TrustedAuthorities`.

Follow-up: migrate the RPC TLS stack to `rustls 0.23` + `tokio-rustls 0.26`.
This is an API-level change (`CertificateDer`, `PrivateKeyDer`,
`WebPkiClientVerifier`) and is tracked separately.

### `cryptoki 0.6.2` and `quick-xml 0.31.0`

These appear in `Cargo.lock` through optional HSM/feature graphs and are **not
built** by the default, `full`, or release Docker configurations. They are not
reachable in the published binary. Follow-up: upgrade or gate the optional HSM
dependency once its API migration is done.

### Unmaintained crates

- `bincode 1.3.3` — used by `neo-consensus` for internal message encoding. No
  drop-in replacement is available; migration to `postcard`/`wincode` requires a
  wire-format change and is deferred.
- `paste 1.0.15` — transitive proc-macro dependency; no runtime exposure.
- `rustls-pemfile 1.0.4` — transitive through the old TLS stack; removed with the
  rustls 0.23 migration.

## Release decision

The published binary does not enable the optional HSM paths, forces HTTP/1.1 on
the server, and does not use the affected client-side TLS features. The
remaining advisories are tracked with the mitigations above.

This is **not** a claim of a clean audit. A production MainNet release should
wait for the hyper 1 and rustls 0.23 migrations and a clean `cargo audit`.
