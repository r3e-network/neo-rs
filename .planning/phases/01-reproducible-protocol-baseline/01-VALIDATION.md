---
phase: 01
slug: reproducible-protocol-baseline
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-13
---

# Phase 01 - Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness through Cargo; Python `unittest` for repository guards |
| **Config file** | Root `Cargo.toml`/`Cargo.lock`, `fuzz/Cargo.toml`/`fuzz/Cargo.lock`, and `scripts/tests/` |
| **Quick run command** | `cargo test --locked -p neo-vm stack_value && cargo test --locked -p neo-execution get_notifications && cargo test --locked -p neo-blockchain state_root::consensus::tests` |
| **Full suite command** | `cargo test --workspace --locked` followed by the Phase 1 locked source, fuzz, supply-chain, workflow, and container gates |
| **Estimated runtime** | Quick sampling under 120 seconds; full phase gate may take 30-60 minutes including a no-cache container build |

---

## Sampling Rate

- **After every task commit:** Run that task's focused Rust or repository guard command.
- **After every plan wave:** Run `cargo check --workspace --tests --locked` plus all affected crate tests and both cargo-deny graphs after dependency changes.
- **Before `$gsd-verify-work`:** The full locked suite and clean Docker smoke test must be green from the final committed source tree.
- **Max feedback latency:** 120 seconds for task-level semantic feedback.

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01 | 1 | PROTO-01 | T-01-01 | Repeated compound IDs reconstruct as one alias-preserving object and notification state follows pre/post-Domovoi immutability rules | unit/integration | `cargo test --locked -p neo-vm stack_value && cargo test --locked -p neo-execution get_notifications` | No - Wave 0 extensions required | pending |
| 01-01-02 | 01 | 1 | PROTO-01, CONSENSUS-01 | T-01-02 | All official activation boundaries select the intended rules and votes never cross version/index/hash identities | unit/adversarial | `cargo test --locked -p neo-config hardfork && cargo test --locked -p neo-blockchain state_root::consensus::tests && cargo test --locked -p neo-execution canonical_execution` | Partial - boundary/index cases missing | pending |
| 01-02-01 | 02 | 2 | BUILD-01 | T-01-03 | Root and fuzz locks resolve only reviewed, non-yanked dependencies under explicit license/source policy | supply-chain/build | `cargo deny check advisories licenses --hide-inclusion-graph && cargo deny --manifest-path fuzz/Cargo.toml check advisories licenses --hide-inclusion-graph && cargo check --manifest-path fuzz/Cargo.toml --locked --all-targets` | Commands exist; policy currently red | pending |
| 01-02-02 | 02 | 2 | BUILD-01 | T-01-04 | CI/fuzz/compatibility failures remain failures, all Cargo resolution is locked, and maintained docs describe the active protocol/build model | lint/repository | `actionlint -no-color && python3 -m unittest scripts.tests.test_dependency_hygiene scripts.tests.test_protocol_target_docs` | Partial - retry/pin/doc guards missing | pending |
| 01-02-03 | 02 | 2 | BUILD-01 | T-01-05 | The exact committed source passes all source gates and builds a runnable image without sibling inputs | full integration/smoke | `cargo fmt --all -- --check && cargo check --workspace --tests --locked && cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings` plus fuzz, deny, script, and Docker gates | Existing infrastructure; final evidence missing | pending |

---

## Wave 0 Requirements

- [ ] Extend `neo-vm/src/tests/stack_item/stack_item.rs` with repeated-ID alias/mutation and conflicting-ID regressions for Array, Struct, Map, and Buffer values.
- [ ] Extend `neo-execution/src/tests/interop/application_engine_runtime.rs` with behavioral pre-Domovoi identity/read-only tests and Domovoi immutable deep-copy tests.
- [ ] Make `neo-config` hardfork coverage table-driven for every scheduled MainNet/TestNet activation boundary.
- [ ] Add a competing-index vote-isolation regression in `neo-blockchain/src/state_root/tests/consensus.rs`.
- [ ] Add structured root/fuzz pin, lockfile, retry-status, dependency-policy, and maintained-document guards under `scripts/tests/`.
- [ ] Add a bincode policy regression proving fuzz no longer resolves it and root resolves it only through the documented consensus-recovery exception while Phase 2 migration remains open.

---

## Manual-Only Verifications

The optional live MainNet/TestNet reference-RPC comparison depends on endpoint
availability. An unavailable endpoint must be retained as unavailable and must
not count as parity evidence. All Phase 1 release-blocking requirements have
automated local or container verification.

---

## Validation Sign-Off

- [x] All planned tasks have automated verification or an explicit Wave 0 dependency.
- [x] Sampling continuity has no three consecutive tasks without automated verification.
- [x] Wave 0 covers every missing test reference identified by research.
- [x] No watch-mode flags are used.
- [x] Task-level feedback latency is targeted below 120 seconds.
- [x] `nyquist_compliant: true` is set in frontmatter.

**Approval:** pending execution and verifier review
