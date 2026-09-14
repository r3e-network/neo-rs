---
kind: external_dependency
name: Tokio async runtime
slug: tokio
category: external_dependency
category_hints:
    - framework_behavior
scope:
    - '**'
---

### Tokio
- Role: Async runtime powering the P2P stack, RPC server, and I/O layers across crates. The workspace pins `tokio = "1.35"` with the `full` feature set; a TODO comment notes reducing per-crate features is desired for faster compilation.
- Durable usage model: All network-bound services (neo-p2p, neo-rpc, neo-node) run under Tokio's reactor; adding new async services should follow the existing pattern of spawning tasks on the shared runtime rather than introducing another executor.