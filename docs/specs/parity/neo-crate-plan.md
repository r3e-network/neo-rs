# Neo Rust Crate Parity Plan

This plan enumerates every root-level `neo-*` crate and records the current parity snapshot, critical gaps, and the next actionable milestone. Update this file whenever work lands so we can trace progress in a document-driven manner.

## How to Work with this Plan

1. **Pick a crate** – read its parity checklist (linked below) plus the roadmap phase.
2. **Update the table** – move the crate's status or next milestone column as soon as work begins or completes.
3. **Implement & test** – run the crate-specific commands from the last column (use `+nightly` when a crate depends on `neo-core`).
4. **Document the outcome** – capture notes, fixtures, and references in the crate's parity doc and in PR descriptions.
5. **Repeat** – advance to the next crate with an 🟥 (blocked) or 🟧 (in progress) status.

_Status legend:_ 🟥 blocked/not started, 🟧 in progress, 🟩 functionally complete (pending polish).

## Crate Status Matrix

| Crate | Parity Spec | Current Snapshot | Critical Gaps | Next Milestone | Tests / Notes |
| --- | --- | --- | --- | --- | --- |
| `neo-base` | [neo-base-parity](./neo-base-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-cli` | *pending* | 🟥 TBD | TBD | TBD | TBD |
| `neo-consensus` | [neo-consensus-parity](./neo-consensus-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-contract` | [neo-contract-parity](./neo-contract-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-core` | *roadmap phase 1/2* | 🟥 TBD | TBD | TBD | TBD |
| `neo-crypto` | [neo-crypto-parity](./neo-crypto-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-node` | *roadmap phase 4* | 🟥 TBD | TBD | TBD | TBD |
| `neo-p2p` | [neo-network-parity](./neo-network-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-proc-macros` | *roadmap phase 1* | 🟥 TBD | TBD | TBD | TBD |
| `neo-rpc` | *roadmap phase 4* | 🟥 TBD | TBD | TBD | TBD |
| `neo-runtime` | [neo-runtime-parity](./neo-runtime-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-store` | *roadmap phase 1* | 🟥 TBD | TBD | TBD | TBD |
| `neo-vm` | [neo-vm-parity](./neo-vm-parity.md) | 🟥 TBD | TBD | TBD | TBD |
| `neo-wallet` | [neo-wallet-parity](./neo-wallet-parity.md) | 🟥 TBD | TBD | TBD | TBD |

## Immediate Focus Queue

1. Identify top priority crate and break goals into sub-milestones.
2. Update this document and the parity roadmap together when status changes.
3. Repeat for the next crate.
