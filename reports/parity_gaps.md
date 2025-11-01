# Neo Rust Node – Parity Audit Notes

This report tracks outstanding gaps and follow-up work needed to keep the `neo-rs`
full node aligned with the canonical C# implementation.

## Persistence / Ledger
- ✔ `NeoSystem::persist_block` now mirrors C# snapshot cloning and per-transaction
  execution, but we still rely on a best-effort plugin emission strategy. Confirm
  plugin expectations around `BlockReceived` vs. `TransactionReceived` once more
  parity tests exist.

## Reverify / Import Pipeline
- ⚠ `Blockchain::handle_reverify` currently deserialises inventories from raw bytes
  every time idle processing runs. The C# actor reuses cached payload objects.
  _Follow-up_: thread payload references (or memoise them) to avoid repeated decode
  work and potential divergence if serialization formats evolve.
- ⚠ Pending regression tests for `handle_import` and `handle_reverify` covering:
  * rejection of out-of-order blocks
  * block verification failures when `Import.verify == true`
  * idle scheduling pauses when headers are buffered

## Memory Pool
- ✔ Header backlog short-circuit behaviour now matches C#, with a unit test in place.
  Additional coverage is still missing for:
  * rebroadcast timing (`BlocksTillRebroadcast` scaling)
  * verification context updates when conflicts are detected

## RPC / Plugin Surface
- Newly restored stub modules (`rpc_get_peers`, `rpc_mempool_*`, etc.) provide type
  parity but remain minimal. Each needs round-trip tests once the RPC client is
  exercised against a full node.

## Next Actions
1. Add actor-level tests for `Blockchain::handle_import` and `handle_reverify`.
2. Profile and optimise inventory re-deserialisation in `handle_reverify`.
3. Expand mempool tests to cover rebroadcast timing and conflict handling.
4. Wire RPC stub modules into live client tests to ensure serde compatibility.

