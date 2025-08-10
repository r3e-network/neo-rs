### Neo-RS Full Node Roadmap

#### Phase 1: Enable RPC and Harden P2P
- Tasks
  - Wire `neo-rpc-server` in `node/src/main.rs` behind config/CLI
  - Implement peer discovery (GetAddr) and improve seed handling
  - Ensure `getconnectioncount`, `getpeers` return live data
- Acceptance
  - TestNet node reaches peers and stays connected > 8 peers; RPC answers basic queries

#### Phase 2: Full Sync Correctness
- Tasks
  - Validate header/blocks pipe end-to-end; add checkpoints
  - Implement snapshot extraction (gzip/zstd) or explicitly document as unsupported
- Acceptance
  - TestNet sync to tip within expected time; maintain sync 24h

#### Phase 3: Consensus (PrivateNet)
- Tasks
  - Feature-gate consensus; wire dBFT engine
  - Provide sample PrivateNet config and keys
- Acceptance
  - PrivateNet produces blocks; transactions execute; RPC reflects progress

#### Phase 4: Tests & Lint Hygiene
- Tasks
  - Fix `crates/mpt_trie` tests to match `neo-core` API
  - Clean up clippy errors or allowlist where justified
- Acceptance
  - `cargo test --all` and clippy pass in CI

#### Phase 5: Ops & Docs
- Tasks
  - Operational runbooks, config matrix, backup/restore docs
  - Observability: metrics and improved health checks
- Acceptance
  - Docs complete; health metrics available


