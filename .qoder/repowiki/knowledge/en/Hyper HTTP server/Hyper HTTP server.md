---
kind: external_dependency
name: Hyper HTTP server
slug: hyper
category: external_dependency
category_hints:
    - vendor_identity
scope:
    - '**'
---

### Hyper
- Role: HTTP server underlying the JSON-RPC endpoint exposed by `neo-node` (default ports 10332 MainNet / 20332 TestNet).
- Durable usage model: Runs atop Tokio; hardened mode (`--rpc-hardened`) disables CORS, requires auth, and restricts methods. Production guidance recommends fronting through a reverse proxy with TLS and rate limits.