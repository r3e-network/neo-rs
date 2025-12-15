# CLI Migration - Development Plan

## Overview
Consolidate duplicate RPC client command code by migrating all client functionality from neo-node to neo-cli, eliminating code duplication and establishing neo-cli as the single canonical RPC client binary.

## Task Breakdown

### Task 1: Command Parity Audit
- **ID**: task-1
- **Description**: Compare all command implementations in neo-node/src/client/commands/*.rs against neo-cli/src/commands/*.rs to identify functional differences, missing features, or implementation discrepancies. Document any commands that exist in neo-node but not in neo-cli, or vice versa. Verify command signatures, error handling patterns, and output formatting consistency.
- **File Scope**:
  - neo-node/src/client/commands/*.rs (21 command files)
  - neo-cli/src/commands/*.rs (28 command files)
  - neo-node/src/client/mod.rs
  - neo-cli/src/main.rs
- **Dependencies**: None
- **Test Command**: `cargo check -p neo-cli && cargo check -p neo-node`
- **Test Focus**:
  - Verify both packages compile successfully
  - Document command coverage gaps (neo-cli has additional commands: send, transfer, vote, wallet, broadcast, export, tools that are not in neo-node)
  - Document command overlap (balance, best_block_hash, block, block_count, block_hash, contract, gas, header, invoke, mempool, native, peers, plugins, relay, state, test_invoke, transfers, tx, validate, version)
  - Identify any behavioral differences in overlapping commands

### Task 2: Merge Missing Commands (if any)
- **ID**: task-2
- **Description**: If the parity audit (task-1) identifies any commands present in neo-node but missing in neo-cli, merge those command implementations into neo-cli. Based on current analysis, neo-cli appears to be a superset with additional commands (wallet, voting, tools), so this task may involve validation rather than actual merging. Ensure all command implementations use consistent error handling patterns and output formatting.
- **File Scope**:
  - neo-cli/src/commands/*.rs (potential additions)
  - neo-cli/src/commands/mod.rs (module declarations)
  - neo-cli/src/main.rs (command routing updates)
- **Dependencies**: depends on task-1
- **Test Command**: `cargo test -p neo-cli --lib && cargo build -p neo-cli --release`
- **Test Focus**:
  - All neo-cli commands compile and execute successfully
  - New or modified commands return correct output formats
  - Error handling is consistent across all commands
  - RPC client integration works correctly

### Task 3: Remove neo-node Client Module
- **ID**: task-3
- **Description**: Delete the entire neo-node/src/client/ directory tree including all command implementations. Update neo-node/src/main.rs to remove client module declaration and any related imports. Clean up neo-node/Cargo.toml by removing neo-rpc-client dependency if it's no longer needed by the node itself (verify node doesn't use RPC client internally). Update import statements and ensure no dead code remains.
- **File Scope**:
  - neo-node/src/client/ (delete entire directory)
  - neo-node/src/main.rs (remove mod client; declaration at line 19)
  - neo-node/Cargo.toml (review and potentially remove neo-rpc-client dependency at line 22)
- **Dependencies**: depends on task-2
- **Test Command**: `cargo build -p neo-node --release && cargo test -p neo-node`
- **Test Focus**:
  - neo-node compiles successfully without client module
  - No broken imports or references to deleted code
  - All neo-node tests pass
  - neo-node binary runs without errors (validate with --check-config)
  - No unused dependencies remain in Cargo.toml

### Task 4: Update Documentation
- **ID**: task-4
- **Description**: Update project documentation to reflect the architectural change. Clarify that neo-cli is the RPC client tool and neo-node is the node daemon. Update README.md to document the separation of concerns. Update any architecture docs (docs/ARCHITECTURE.md, docs/DEPLOYMENT.md, docs/OPERATIONS.md) that reference client commands or the old neo-node client module. Add usage examples showing how to use neo-cli to query a neo-node instance.
- **File Scope**:
  - README.md
  - docs/ARCHITECTURE.md
  - docs/DEPLOYMENT.md
  - docs/OPERATIONS.md
  - neo-cli/README.md (create if missing)
  - neo-node/README.md (update if exists)
- **Dependencies**: depends on task-3
- **Test Command**: `cargo doc --no-deps -p neo-cli -p neo-node && cargo build --all-targets`
- **Test Focus**:
  - Documentation builds without warnings
  - No broken documentation links
  - Examples in documentation are accurate and runnable
  - Architecture diagrams/descriptions reflect current structure
  - Usage instructions are clear and correct

### Task 5: Integration Testing
- **ID**: task-5
- **Description**: Perform end-to-end validation of the migration. Start a neo-node instance (testnet or local), execute all neo-cli commands against it to verify full functionality. Test both success and error paths (invalid addresses, non-existent blocks, etc.). Verify output formats (JSON, table, plain) work correctly. Confirm neo-node runs independently without client module. Run full test suite for both packages.
- **File Scope**:
  - neo-cli/src/**/*.rs (all command implementations)
  - neo-node/src/**/*.rs (node daemon code)
  - Test scripts or integration test suites
- **Dependencies**: depends on task-4
- **Test Command**: `cargo test --all && cargo build --release -p neo-cli -p neo-node`
- **Test Focus**:
  - All unit tests pass for both neo-cli and neo-node packages
  - neo-cli can successfully connect to and query a running neo-node
  - All neo-cli commands execute successfully against live node
  - Error handling works correctly (network failures, invalid inputs)
  - Output formatting (json/table/plain) works for all commands
  - neo-node runs as standalone daemon without client dependencies
  - No regression in node functionality after removing client module

## Acceptance Criteria
- [ ] Command parity audit completed with documented findings (task-1)
- [ ] neo-cli contains all necessary RPC client commands (task-2)
- [ ] neo-node/src/client/ directory completely removed (task-3)
- [ ] neo-node/Cargo.toml cleaned up (no unused dependencies) (task-3)
- [ ] neo-node compiles and runs without client module (task-3)
- [ ] Documentation updated to reflect architecture (task-4)
- [ ] All unit tests pass: `cargo test -p neo-cli && cargo test -p neo-node` (task-5)
- [ ] Integration test: neo-cli successfully queries running neo-node (task-5)
- [ ] Code coverage ≥90% for both packages
- [ ] Release builds succeed: `cargo build --release -p neo-cli -p neo-node`
- [ ] No compilation warnings in either package

## Technical Notes
- **Code Duplication Analysis**: Initial analysis shows that neo-cli and neo-node share 20 identical command implementations (balance, best_block_hash, block, block_count, block_hash, contract, gas, header, invoke, mempool, native, peers, plugins, relay, state, test_invoke, transfers, tx, validate, version). The implementations appear functionally identical (see balance.rs comparison).
- **neo-cli Superset**: neo-cli has 8 additional commands not present in neo-node's client: send, transfer, vote, wallet (with subcommands), broadcast (with subcommands), export, tools (with parse/sign utilities). This suggests neo-cli is the more complete implementation.
- **Dependency Cleanup**: neo-node uses neo-rpc-client at line 22 of Cargo.toml. Verify whether neo-node needs RPC client for internal operations (health checks, metrics) or if it's solely for the client module being removed.
- **Architecture Clarity**: Post-migration, the architecture becomes clearer:
  - neo-node: Blockchain node daemon with P2P, consensus, storage, RPC server
  - neo-cli: RPC client tool for querying and interacting with neo-node instances
- **Backward Compatibility**: Ensure neo-node users who were using embedded client commands are informed to use neo-cli instead. Consider adding a deprecation notice or migration guide.
- **Testing Strategy**: Priority test paths:
  1. Verify neo-node runs independently (start/stop/sync)
  2. Verify neo-cli can connect to neo-node
  3. Test representative commands from each category (node info, blockchain queries, token ops, contract invocation)
  4. Test error scenarios (offline node, invalid params)
