# neo-gui — Native Neo Node Manager

A beautiful, native **Rust desktop application** for managing a [neo-rs](../README.md)
Neo N3 node. Built with [`egui`](https://github.com/emilk/egui)/`eframe`.

It is a **pure client**: it talks to a running node over JSON-RPC and (for local
nodes) supervises the `neo-node` process. It links no node-internal crates, so it
builds and ships independently of the workspace (it is a `workspace.exclude`
member — build it from this directory).

## Run

```bash
cd neo-gui
cargo run --release
```

Then set your node's RPC endpoint in **Settings** (or use a preset), and the
dashboard begins polling.

## Screens

| Screen | What it does |
|--------|--------------|
| **Dashboard** | Live status cards (height, headers, peers, mempool, network, validators), a sync progress bar + height chart, and protocol info. |
| **Network** | Connected and known peer tables, connection counts. |
| **Wallet** | Open a NEP-6 wallet on the node and list its accounts/balances (requires wallet RPC enabled). |
| **Contracts** | An RPC explorer — invoke any JSON-RPC method (with quick-picks for common ones) and inspect the JSON result. |
| **Node** | Start/stop and supervise a local `neo-node` process; tail its logs live. |
| **Signer** | The validator key-management backend — software, AWS CloudHSM, Azure, Google Cloud HSM, or AWS Nitro Enclave (with the node config snippet for each). |
| **Settings** | RPC endpoint, presets, and polling behaviour. |

## Design

- A background thread polls the node every few seconds into shared state; the UI
  renders that state each frame. One-off actions (RPC calls, wallet ops) run on
  worker threads — the UI thread never blocks on the network.
- The signer screen mirrors the multi-cloud HSM / Nitro-TEE design
  (`claudedocs/multicloud-hsm-design.md`, `claudedocs/aws-hsm-nitro-tee-design.md`).
