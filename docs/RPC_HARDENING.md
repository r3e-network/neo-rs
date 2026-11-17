# RPC Hardening

Recommendations and a sample `RpcServer.json` for running the RPC plugin securely.

## Placement
- `RpcServer.json` lives under `Plugins/RpcServer` relative to the config directory. In Docker/Compose, set `NEO_PLUGINS_DIR` to a writable, persistent path (defaults to `/data/Plugins`).

## Recommendations
- Bind to loopback (`127.0.0.1`) and front RPC with a reverse proxy (TLS/auth/rate limits) if exposing beyond localhost.
- Disable CORS unless you have a trusted origin list; prefer an allowlist instead of `*`.
- Set strong `rpc_user`/`rpc_pass` credentials if you cannot proxy-authenticate.
- Wallet RPCs (`openwallet`, `closewallet`, `dumpprivkey`, `importprivkey`, `getnewaddress`, `listaddress`, `getwalletbalance`, `getwalletunclaimedgas`, `calculatenetworkfee`, `sendfrom`, `sendtoaddress`, `sendmany`, `canceltransaction`) are refused with HTTP 401 unless `rpc_user` (`RpcUser`) and a non-empty `rpc_pass` (`RpcPass`) are configured and the request authenticates with them. This is stricter than the C# node, which serves them unauthenticated when no credentials are set. A loopback bind, TLS or a proxy in front does not lift the requirement; a node that uses wallet RPCs must configure both settings.
- Set sensible limits: `max_concurrent_connections`, `max_request_body_size`, `max_gas_invoke`, `max_fee`, `max_iterator_result_items`, `max_stack_size`.
- Optional built-in per-IP rate limiting is available via `max_requests_per_second` and `rate_limit_burst` (disabled when set to 0). Use a reverse proxy limiter for stronger guarantees.
- When credentials are configured, every request except a CORS preflight is authenticated from its headers before the body is read or parsed; a request without valid credentials gets HTTP 401 with a null JSON-RPC id and the connection is closed. Ahead of that, each peer IP is held to a separate admission budget, `admission_requests_per_second` / `admission_burst` (HTTP 429 when exceeded). It is never drawn from the per-method budget above. Left unset it is four times `max_requests_per_second` / `rate_limit_burst` and is off when those are 0; set `admission_requests_per_second` to 0 to turn it off on its own. The peer is always the TCP peer address (forwarding headers are ignored), so behind a reverse proxy all clients share the proxy's budget.
- A POST body must arrive within `request_body_timeout` seconds in total (default 120) and must not stall for longer than `request_body_idle_timeout` seconds (default 30); either set to 0 is off. A connection that misses a deadline is closed without a response, which frees its `max_concurrent_connections` slot. The deadlines cover receiving the body only, never the execution of the call. The total deadline bounds how slowly a body may be sent: a maximum-size transaction (about 137 KB as a `sendrawtransaction` body) needs about 1.2 KB/s, a body at the default 5 MiB `max_request_body_size` about 44 KB/s; raise `request_body_timeout` if clients on slower links post bodies that large.
- Keep `disabled_methods` populated for any RPC methods you do not need.
- Prefer environment overrides for secrets and endpoints in containers: `NEO_RPC_USER`, `NEO_RPC_PASS`, `NEO_RPC_TLS_CERT`, `NEO_RPC_TLS_PASS`, `NEO_RPC_BIND`, `NEO_RPC_PORT`, `NEO_RPC_ALLOW_ORIGINS`, `NEO_RPC_DISABLED_METHODS`.
- Use the CLI `--rpc-hardened` switch to force auth, disable CORS, and disable wallet/key-export/send methods (`openwallet`, `dumpprivkey`, `importprivkey`, `sendfrom`, `sendto`, `sendmany`, `sendrawtransaction`, `listplugins`) at startup; this will also fail if credentials are missing.
- Built-in TLS is supported via `ssl_cert` / `ssl_cert_password` / `trusted_authorities` (or `NEO_RPC_TLS_*`). Prefer terminating TLS at a reverse proxy when possible; if `TrustedAuthorities` is set, at least one matching client CA root must load or the server refuses to start.
- Expose only what you need: avoid `listplugins` and `openwallet` on untrusted networks; keep them disabled or restrict via proxy ACLs.
- Compatibility note: set `NEO_LISTPLUGINS_COMPAT=fixture` (and optionally `NEO_PLUGIN_VERSION=3.0.0.0`) to emulate legacy C# `listplugins` output when needed for fixture-based clients.
- Default plugin set is limited to the stable services (rpc-server, rocksdb-store, application-logs, tokens-tracker). `tokens-tracker` is optional and only exposes RPC methods when enabled. `state-service` is optional and only enabled when configured (or via `--state-root`). Consensus (dBFT) is not wired into `neo-node` yet.
- Keep `/healthz` bound to localhost by default (`--health-port`/`NEO_HEALTH_PORT`); if you proxy it, ensure it stays internal.
- Consider setting `--health-max-header-lag` to fail health checks on large sync gaps.

## Sample `Plugins/RpcServer/RpcServer.json`
This example is for TestNet (magic `894710606`, port `20332`). Adjust `network`, `port`, and credentials for MainNet or your network.

```json
{
  "PluginConfiguration": {
    "Servers": [
      {
        "network": 894710606,
        "bind_address": "127.0.0.1",
        "port": 20332,
        "ssl_cert": "",
        "ssl_cert_password": "",
        "trusted_authorities": [],
        "max_concurrent_connections": 40,
        "max_requests_per_second": 0,
        "rate_limit_burst": 0,
        "max_request_body_size": 5242880,
        "rpc_user": "change-me",
        "rpc_pass": "change-me-strongly",
        "enable_cors": false,
        "allow_origins": [],
        "keep_alive_timeout": 60,
        "request_headers_timeout": 15,
        "max_gas_invoke": 2000000000,
        "max_fee": 10000000,
        "max_iterator_result_items": 100,
        "max_stack_size": 65535,
        "disabled_methods": ["openwallet", "listplugins"],
        "session_enabled": false,
        "session_expiration_time": 60,
        "find_storage_page_size": 50
      }
    ],
    "UnhandledExceptionPolicy": "Ignore"
  }
}
```

You can copy `config/Plugins/RpcServer/RpcServer.json.example` into your plugin directory and adjust it for your environment.

## Reverse proxy (outline)
- Terminate TLS and perform auth/rate-limiting at the proxy (Nginx/Caddy/Envoy).
- Allow only the needed RPC methods/paths, and optionally IP-restrict.
- Ensure the proxy forwards to the bind address/port configured above (`127.0.0.1:20332` in the sample).
