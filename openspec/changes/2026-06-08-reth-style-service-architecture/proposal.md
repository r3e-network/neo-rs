# Reth-style service architecture for Neo

## Why

The current `neo-actors` crate is an Akka-style actor framework directly ported from the C# Neo implementation. This was a mistake. Rust's idiomatic concurrency model is `async` + `tokio` + channels, not actors. The C# actor model creates a Cargo cycle. Reth and polkadot-sdk both use a service-based architecture.

**We are re-architecting to use reth's pattern.**

## Reth architecture (study notes)

| Reth trait | Neo equivalent | Purpose |
|---|---|---|
| BlockExecutor | BlockExecutor | Execute a block |
| BlockReader | BlockReader | Read block data from storage |
| StateProviderFactory | StateProviderFactory | Get state at a block |
| TransactionPool | MempoolService | Manage pending transactions |
| NetworkManager | NetworkService | P2P networking |
| Consensus | ConsensusService | dBFT consensus |
| Engine | NeoEngine | Engine API |

## Service pattern

```rust
#[async_trait::async_trait]
pub trait BlockExecutor: Send + Sync {
    async fn execute(&self, block: &Block) -> Result<ExecutionOutcome>;
}
```

## What gets replaced

| Old (actor) | New (service) |
|---|---|
| ActorSystem | Node builder |
| ActorRef | mpsc::Sender<T> (cheap to clone) |
| actor_cast! | service.method(...).await |
| Mailbox dispatch | tokio::select! over channels |
| Ask pattern | request/response via oneshot channel |

## Migration plan

### Stage A: Define service traits in `neo-runtime`
### Stage B: Rewrite `neo-blockchain` as a service
### Stage C: Rewrite `neo-network` (LocalNode, RemoteNode, TaskManager) as services
### Stage D: Rewrite `neo-system` (NeoSystem) as Node builder
### Stage E: Update all consumers
### Stage F: Remove `neo-actors`

## Verification

- cargo check --workspace - 0 errors
- cargo test --workspace --lib - all tests pass
- neo-actors directory does not exist
- No actor macros, no ActorRef, no Actor trait
