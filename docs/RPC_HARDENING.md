# RPC Hardening

Recommendations for running the built-in `neo-node` JSON-RPC server securely.

## Current Integration

The current daemon consumes `[rpc] enabled`, `bind_address`, and `port` from the
TOML node config. Deeper `neo-rpc` settings such as basic auth, CORS allowlists,
disabled methods, and request limits are present in the RPC crate but are not yet
wired through `neo-node` startup.

## Recommendations
- Bind to loopback (`127.0.0.1`) and front RPC with a reverse proxy (TLS/auth/rate limits) if exposing beyond localhost.
- Terminate TLS at the reverse proxy or tunnel.
- Enforce authentication, method allowlists, request-size limits, and rate limits at the proxy.
- Do not publish wallet-mutating methods on untrusted networks.
- Compatibility note: set `NEO_LISTPLUGINS_COMPAT=fixture` (and optionally `NEO_PLUGIN_VERSION=3.0.0.0`) to emulate legacy C# `listplugins` output when needed for fixture-based clients.
- Use JSON-RPC probes such as `getversion` and `getblockcount` for health checks; the daemon does not currently expose `/healthz`.

## Example TOML

```toml
[rpc]
enabled = true
bind_address = "127.0.0.1"
port = 10332
```

## Reverse proxy (outline)
- Terminate TLS and perform auth/rate-limiting at the proxy (Nginx/Caddy/Envoy).
- Allow only the needed RPC methods/paths, and optionally IP-restrict.
- Ensure the proxy forwards to the bind address/port configured above (`127.0.0.1:10332` in the sample).
