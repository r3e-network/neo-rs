# neo-rs Documentation

`neo-rs` is a full Neo N3 blockchain node implemented from scratch in Rust, with
byte-for-byte protocol parity to the official C# reference node (tracked through
Neo v3.10.0). The runnable program is a single daemon, `neo-node`: it syncs the
chain over a custom TCP P2P protocol, executes NeoVM bytecode and native
contracts, maintains the MPT state root, and optionally serves a JSON-RPC API.

## Start here

If you want to **run a node**, begin with [getting-started.md](./getting-started.md)
and keep [configuration.md](./configuration.md) open alongside it. If you want to
**understand how the node works**, start with [architecture.md](./architecture.md)
and follow it into [dataflow.md](./dataflow.md). The navigation table below maps
every page to what you will learn from it.

## Navigation

| Doc | What you'll learn |
|-----|-------------------|
| [getting-started.md](./getting-started.md) | Install prerequisites, build `neo-node`, run a TestNet or MainNet node, point it at a data directory, and smoke-test it over JSON-RPC. Includes Docker and Makefile shortcuts. |
| [configuration.md](./configuration.md) | Every TOML section and key the daemon reads (`[network]`, `[storage]`, `[p2p]`, `[rpc]`, `[consensus]`, `[blockchain]`, `[mempool]`), preset-plus-override behavior, environment variables, and which shipped keys are parsed but not yet consumed. |
| [operations.md](./operations.md) | Running in production: systemd and Docker deployment, storage sizing, health checks via RPC, observability, security hardening, backups, upgrades, and incident response. |
| [rpc-api.md](./rpc-api.md) | The JSON-RPC 2.0 surface (~55 methods) grouped by domain — blockchain, smart-contract invocation, state and MPT proofs, node/network, wallet, plugins — with parameters, request/response shape, and curl examples. |
| [architecture.md](./architecture.md) | The 27-crate, four-layer design (Foundation → Protocol/VM/State → Service → Application), a crate reference table, and the key design decisions (two-tier VM, async services, single error type, pluggable storage, C# parity). |
| [dataflow.md](./dataflow.md) | How data and control move at runtime: startup/composition, block ingestion, transaction lifecycle, a dBFT consensus round, RPC request handling, and the state/storage overlay model — each with a diagram. |
| [protocol-compatibility.md](./protocol-compatibility.md) | What "byte-for-byte C# parity" means, the 11 native contracts, the 7 hardforks with MainNet/TestNet activation heights, supported subsystems (consensus, VM, NEP standards, P2P), and the cryptography stack. |

## Learning paths

### New operator (run and maintain a node)

1. [getting-started.md](./getting-started.md) — build, run, and smoke-test a node.
2. [configuration.md](./configuration.md) — tune the TOML for your network and storage.
3. [operations.md](./operations.md) — deploy, monitor, harden, back up, and upgrade.

For the methods you will query while operating, see
[rpc-api.md](./rpc-api.md). Deployment, systemd, Docker, and RPC hardening all
live in [operations.md](./operations.md).

### New developer (understand the system)

1. [architecture.md](./architecture.md) — the crates, layers, and design decisions.
2. [dataflow.md](./dataflow.md) — how blocks, transactions, and queries flow through the services.
3. [protocol-compatibility.md](./protocol-compatibility.md) — the protocol surface, native contracts, and hardforks the node must match.
4. [rpc-api.md](./rpc-api.md) — the external interface clients use to drive the node.

## Documentation map

```mermaid
flowchart TD
    Index["docs/README.md<br/>(this index)"]

    subgraph Operator["Operator path"]
        GS[getting-started.md]
        CFG[configuration.md]
        OPS[operations.md]
    end

    subgraph Developer["Developer path"]
        ARCH[architecture.md]
        FLOW[dataflow.md]
        PROTO[protocol-compatibility.md]
    end

    RPC[rpc-api.md]

    Index --> GS
    Index --> ARCH
    GS --> CFG --> OPS
    ARCH --> FLOW --> PROTO
    OPS --> RPC
    PROTO --> RPC
```
