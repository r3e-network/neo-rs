# Architecture Cleanup - Development Plan

## Overview
Restructure the neo-rs workspace to eliminate deprecated crates, fix circular dependencies, and establish a clean layered architecture with proper dependency boundaries.

## Task Breakdown

### Task 1: Remove Deprecated Crates
- **ID**: task-1
- **Description**: Delete neo-akka/ and neo-plugins/ directories (merged functionality into neo-core), and remove all workspace references to these crates
- **File Scope**:
  - `neo-akka/**` (entire directory for deletion)
  - `neo-plugins/**` (entire directory for deletion)
  - `Cargo.toml` (workspace members list, lines 43, 51-53)
  - `Cargo.toml` (workspace.dependencies section, lines 127-131)
- **Dependencies**: None
- **Test Command**: `cd /home/neo/git/neo-rs && cargo build --workspace 2>&1 | grep -E '(error|warning:.*neo-akka|warning:.*neo-plugins)'`
- **Test Focus**:
  - Verify no references to neo-akka or neo-plugins remain
  - Confirm workspace compiles without these crates
  - Check that no broken imports exist in other crates

### Task 2: Fix neo-rpc-client Dependencies
- **ID**: task-2
- **Description**: Remove neo-core and neo-vm dependencies from neo-rpc-client, replacing with lightweight foundation crates (neo-primitives, neo-io, neo-json)
- **File Scope**:
  - `neo-rpc-client/Cargo.toml` (dependencies section, lines 9-13)
  - `neo-rpc-client/src/**/*.rs` (imports and type usage)
- **Dependencies**: None
- **Test Command**: `cd /home/neo/git/neo-rs && cargo test -p neo-rpc-client --lib 2>&1`
- **Test Focus**:
  - RPC client builds without neo-core dependency
  - All type references use neo-primitives instead of neo-core re-exports
  - Network protocol serialization still works correctly
  - HTTP client functionality remains intact

### Task 3: Fix neo-cli Dependencies
- **ID**: task-3
- **Description**: Remove heavy neo-core dependency from neo-cli, ensuring it only depends on neo-rpc-client, neo-json, clap, and tokio for a lightweight client binary
- **File Scope**:
  - `neo-cli/Cargo.toml` (dependencies section, line 16)
  - `neo-cli/src/**/*.rs` (imports and RPC interaction code)
- **Dependencies**: depends on task-2
- **Test Command**: `cd /home/neo/git/neo-rs && cargo build -p neo-cli --release && ./target/release/neo-cli --help`
- **Test Focus**:
  - CLI binary compiles without neo-core
  - All commands work via neo-rpc-client
  - Binary size is significantly reduced (target <10MB)
  - Help text and command parsing still functional

### Task 4: Refactor neo-core as Thin Aggregation Layer
- **ID**: task-4
- **Description**: Move duplicated cryptography code to neo-crypto and storage code to neo-storage, using re-exports in neo-core to maintain backward compatibility while reducing direct dependencies
- **File Scope**:
  - `neo-core/Cargo.toml` (dependencies section, lines 26-36 for crypto duplication)
  - `neo-core/src/cryptography/**` (migrate to neo-crypto or convert to re-exports)
  - `neo-core/src/persistence/**` (migrate storage abstractions to neo-storage)
  - `neo-crypto/src/**` (destination for consolidated crypto code)
  - `neo-storage/src/**` (destination for consolidated storage code)
- **Dependencies**: depends on task-1
- **Test Command**: `cd /home/neo/git/neo-rs && cargo test --workspace --lib --exclude neo-cli --exclude neo-node 2>&1 | tail -50`
- **Test Focus**:
  - All neo-core tests pass with re-exported symbols
  - No circular dependencies introduced
  - Crypto functions work identically after migration
  - Storage traits accessible through both neo-core and neo-storage

### Task 5: Unify Version Management
- **ID**: task-5
- **Description**: Update all crate Cargo.toml files to use `version.workspace = true` instead of hardcoded versions for consistency and single-source version management
- **File Scope**:
  - `neo-io/Cargo.toml` (line 2: change version = "0.7.0" to version.workspace = true)
  - `neo-core/Cargo.toml` (line 3: change version = "0.6.0" to version.workspace = true)
  - `neo-cli/Cargo.toml` (line 3: change version = "0.6.0" to version.workspace = true)
  - `neo-node/Cargo.toml` (line 3: change version = "0.6.0" to version.workspace = true)
  - `neo-rpc-client/Cargo.toml` (line 3: change version = "0.6.0" to version.workspace = true)
  - `neo-vm/Cargo.toml` (line 3: change version = "0.7.0" to version.workspace = true)
  - `neo-tee/Cargo.toml` (line 3: change version = "0.6.0" to version.workspace = true)
- **Dependencies**: None
- **Test Command**: `cd /home/neo/git/neo-rs && cargo build --workspace && cargo --version && cargo metadata --format-version 1 | jq '.packages[] | select(.name | startswith("neo-")) | {name, version}' 2>&1`
- **Test Focus**:
  - All crates report version 0.7.0 (workspace version)
  - Cargo.lock reflects unified versioning
  - No version conflicts in dependency resolution
  - Workspace builds successfully with unified versions

## Acceptance Criteria
- [ ] neo-akka/ and neo-plugins/ directories completely removed
- [ ] neo-cli binary is lightweight (<10MB) and depends only on neo-rpc-client, neo-json, clap, tokio
- [ ] neo-rpc-client has no neo-core dependency, uses only neo-primitives, neo-io, neo-json
- [ ] neo-core acts as thin aggregation layer with re-exports from foundation crates
- [ ] Duplicated crypto code consolidated in neo-crypto
- [ ] Duplicated storage code consolidated in neo-storage
- [ ] All workspace crates use `version.workspace = true`
- [ ] All unit tests pass: `cargo test --workspace --lib`
- [ ] Workspace builds successfully: `cargo build --workspace`
- [ ] Code coverage ≥90% maintained after refactoring
- [ ] No circular dependencies exist between crates
- [ ] Binary sizes reduced for neo-cli and neo-node

## Technical Notes
- **Backward Compatibility**: neo-core re-exports maintain API compatibility for downstream crates during migration period
- **Parallelization**: Task 1, Task 2, and Task 5 are fully independent and can run in parallel; Task 3 depends on Task 2; Task 4 depends on Task 1
- **Dependency Graph Validation**: Use `cargo tree -p neo-cli` and `cargo tree -p neo-rpc-client` to verify dependency cleanup
- **Build Verification**: After each task, run `cargo check --workspace` to catch broken references early
- **Migration Strategy**: For Task 4, create re-export modules first, then migrate implementations, then remove duplicates to minimize breakage
- **Version Alignment**: The workspace uses version 0.7.0; crates at 0.6.0 should be updated to workspace version in Task 5
- **Critical Files**:
  - Workspace Cargo.toml controls all version and dependency declarations
  - neo-core/src/lib.rs will need extensive re-export declarations after Task 4
  - neo-rpc-client likely uses UInt160, UInt256 from neo-core which must switch to neo-primitives
- **Testing Focus**: Integration tests in neo-core/tests/ must continue passing after refactoring, especially p2p_message_tests.rs and integration_tests.rs
