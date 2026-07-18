# Phase 1 Single-VM Cutover Evidence

**Captured:** 2026-07-14
**Verdict:** Single-VM migration and locked workspace source gates passed

## Architecture Checks

| Check | Result |
|---|---|
| Canonical interpreter | `neo-execution` calls only the local `neo-vm` engine |
| Runtime value | `StackItem` is used directly by execution and interoperable projections |
| RPC client boundary | Remote values use immutable `RpcStackItem` |
| Dependency graph | Root metadata resolves local `neo-vm`; no external VM package |
| Fuzz dependency | `fuzz/Cargo.toml` resolves `../neo-vm` |
| Git-source policy | `deny.toml` allows no Git source |
| Port provenance | `neo-vm/THIRD_PARTY_NOTICES.md` retains the MIT notice |

## Verification Results

| Gate | Result |
|---|---|
| `cargo test -p neo-vm --lib` | 157 passed during the cutover |
| Current `neo-vm` regression suite | 166 passed in the locked all-target workspace run |
| `cargo test -p neo-manifest` | 58 passed |
| `cargo test -p neo-payloads` | 121 unit tests and 1 doctest passed |
| `cargo test -p neo-execution` | 115 passed; 2 doctests ignored |
| `cargo test -p neo-native-contracts` | 360 unit tests and 23 manifest tests passed; 3 doctests ignored |
| `cargo test -p neo-node --bin neo-db-probe` | 40 passed |
| RPC client/server migration suites | 277 client and 372 server tests passed |
| RPC max-size regression | focused test passed with exact C# error text |
| `python3 -m unittest scripts.tests.test_dependency_hygiene` | 9 passed |
| `cargo check --workspace --all-targets` | passed |
| `cargo test --workspace --all-targets --profile test --locked` | passed |
| `cargo clippy --workspace --all-targets --profile test --locked -- -D warnings` | passed |
| Root and fuzz locked `cargo deny` policy | passed; informational warnings only |
| Rust 1.89 fuzz all-target check | passed |
| Full Python repository policy suite | 344 passed |
| Active OpenSpec strict validation | passed |
| `cargo fmt --all -- --check` | passed after migration formatting |
| `git diff --check` | passed |

The pinned execution-spec corpus passed 405 of 405 vectors against a fresh
local MainNet-configured node. The live v3.10.1 consistency run separately
exited 75 because its required C#/NeoGo reference pair was unavailable; it was
correctly recorded as unevaluated rather than parity success.

## Evidence Limits

The focused tests prove that direct `StackItem` ownership compiles and retains
the covered object, codec, notification, RPC, and database semantics. They do
not prove every official Neo v3.10.1 execution transition or a complete chain
replay. The follow-up audit now retains 29 pinned C# hardfork observations.
Full MainNet state-root replay in Phase 5 remains the authoritative
chain-parity gate.
