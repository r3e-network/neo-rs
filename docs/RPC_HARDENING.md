# RPC Hardening

Recommendations and a sample `RpcServer.json` for running the RPC plugin securely.

## Placement
- `RpcServer.json` lives under `Plugins/RpcServer` relative to the config directory. In Docker/Compose, set `NEO_PLUGINS_DIR` to a writable, persistent path (defaults to `/data/Plugins`).

## Recommendations
- Bind to loopback (`127.0.0.1`) and front RPC with a reverse proxy (TLS/auth/rate limits) if exposing beyond localhost.
- Disable CORS unless you have a trusted origin list; prefer an allowlist instead of `*`.
- Set strong `rpc_user`/`rpc_pass` credentials if you cannot proxy-authenticate.
- Set sensible limits: `max_concurrent_connections`, `max_request_body_size`, `max_gas_invoke`, `max_fee`, `max_iterator_result_items`, `max_stack_size`.
- Optional built-in per-IP rate limiting is available via `max_requests_per_second` and `rate_limit_burst` (disabled when set to 0). Use a reverse proxy limiter for stronger guarantees.
- Keep `disabled_methods` populated for any RPC methods you do not need.
- Prefer environment overrides for secrets and endpoints in containers: `NEO_RPC_USER`, `NEO_RPC_PASS`, `NEO_RPC_TLS_CERT`, `NEO_RPC_TLS_PASS`, `NEO_RPC_BIND`, `NEO_RPC_PORT`, `NEO_RPC_ALLOW_ORIGINS`, `NEO_RPC_DISABLED_METHODS`.
- Use the CLI `--rpc-hardened` switch to force auth, disable CORS, and disable `openwallet`/`listplugins` at startup; this will also fail if credentials are missing.
- TLS termination is **not supported** by the Rust RPC plugin. Always terminate TLS at a reverse proxy or tunnel; setting `ssl_cert`/`ssl_cert_password`/`trusted_authorities` will cause the server to refuse to start.
- Expose only what you need: avoid `listplugins` and `openwallet` on untrusted networks; keep them disabled or restrict via proxy ACLs.
- Default plugin set is limited to the stable plugins (dbft, rpc-server, rocksdb-store, application-logs, sqlite-wallet). `tokens-tracker` is currently a stub and will log a warning if loaded.
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
