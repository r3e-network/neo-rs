# Client CLI Integration - Development Plan

## Overview
Integrate RPC client commands directly into the neo-node CLI, enabling users to execute blockchain queries and operations without running a separate neo-cli binary.

## Task Breakdown

### Task 1: Add Clap Subcommand Plumbing to main.rs
- **ID**: task-1
- **Description**: Extend NodeCli struct to accept optional subcommands. Create a NodeCommand enum wrapping RpcCommands, add early dispatch logic in main() to detect and route client commands before NodeConfig::load(). Return immediately after command execution to prevent node startup.
- **File Scope**: neo-node/src/main.rs (lines 50-237)
- **Dependencies**: None
- **Test Command**: `cargo test -p neo-node --lib -- --test-threads=1`
- **Test Focus**:
  - CLI parsing accepts both daemon flags and client subcommands
  - Subcommand presence is correctly detected
  - Client commands do not trigger node initialization

### Task 2: Implement ClientCli Argument Parsing
- **ID**: task-2
- **Description**: Create a ClientCli struct to encapsulate global RPC connection parameters (--rpc-url, --rpc-user, --rpc-pass, --output). Map these arguments to ClientConfig. Add conversion logic to transform ClientCli + RpcCommands into a fully configured ClientConfig instance. Ensure default values (rpc_url: "http://127.0.0.1:10332") are applied correctly.
- **File Scope**: neo-node/src/main.rs (new struct definition after NodeCli), neo-node/src/client/mod.rs (ClientConfig already exists at lines 31-53)
- **Dependencies**: None
- **Test Command**: `cargo test -p neo-node --lib client_config -- --test-threads=1`
- **Test Focus**:
  - ClientCli parses global flags correctly
  - Defaults are applied when flags omitted
  - ClientConfig creation succeeds with valid and invalid URLs
  - Environment variables override defaults

### Task 3: Wire Client Command Execution in main()
- **ID**: task-3
- **Description**: Add conditional branch in main() after NodeCli::parse() to detect client subcommands. If present, build ClientConfig from ClientCli args, invoke execute_command() asynchronously, print output to stdout, and exit with appropriate status code (0 for success, 1 for errors). Ensure error messages are formatted consistently with existing node errors.
- **File Scope**: neo-node/src/main.rs (lines 234-237, immediate post-parse logic)
- **Dependencies**: depends on task-1, task-2
- **Test Command**: `cargo test -p neo-node --lib client_execution -- --test-threads=1 && cargo build -p neo-node --release`
- **Test Focus**:
  - Client commands execute without node initialization
  - Output is printed to stdout correctly
  - Error handling produces user-friendly messages
  - Exit codes are correct (0=success, 1=error)

### Task 4: Update neo-node README.md Documentation
- **ID**: task-4
- **Description**: Add a new "Client Commands" section documenting the integrated RPC client functionality. Include usage examples (e.g., `neo-node --rpc-url http://seed1.neo.org:10332 block 12345`), list all available subcommands from RpcCommands enum, document global client flags (--rpc-url, --rpc-user, --rpc-pass, --output), and explain the dual-mode operation (daemon vs client).
- **File Scope**: neo-node/README.md (insert after line 40, before "## Command-line Options")
- **Dependencies**: depends on task-3
- **Test Command**: `cargo doc -p neo-node --no-deps --open`
- **Test Focus**:
  - Documentation is accurate and consistent with implementation
  - Examples execute successfully
  - All client flags are documented
  - Usage patterns are clear

### Task 5: Add CLI Parsing and Integration Tests
- **ID**: task-5
- **Description**: Add unit tests in main.rs tests module verifying NodeCli parses client subcommands, ClientCli defaults are applied, and subcommand dispatch logic routes correctly. Create integration test in neo-node/tests/client_integration_tests.rs to execute actual client commands against a mock RPC server (using wiremock or similar). Test error cases (invalid URL, missing RPC server, auth failures).
- **File Scope**: neo-node/src/main.rs (expand #[cfg(test)] mod tests at line 841), neo-node/tests/client_integration_tests.rs (new file)
- **Dependencies**: depends on task-3
- **Test Command**: `cargo test -p neo-node -- --test-threads=1`
- **Test Focus**:
  - CLI parsing handles all client command variants
  - Integration tests verify end-to-end client command flow
  - Mock RPC responses are handled correctly
  - Error cases produce expected outcomes
  - Coverage of parsing, execution, and error paths ≥90%

## Acceptance Criteria
- [ ] `neo-node version --rpc-url http://localhost:10332` executes without starting the node daemon
- [ ] All RpcCommands enum variants are accessible via CLI subcommands
- [ ] Global client flags (--rpc-url, --rpc-user, --rpc-pass, --output) are parsed and applied
- [ ] Client commands exit immediately after execution (no node startup)
- [ ] Error messages are user-friendly and consistent with existing node errors
- [ ] README.md documents all client commands and usage patterns
- [ ] All unit tests pass
- [ ] Code coverage ≥90% for new client CLI code paths

## Technical Notes
- **Clap Subcommand Architecture**: Use `#[command(subcommand)]` attribute on a new `Option<NodeCommand>` field in NodeCli. NodeCommand is an enum wrapping RpcCommands plus a ClientCli struct for global flags.
- **Early Dispatch Pattern**: Check for subcommand presence immediately after `NodeCli::parse()` in main(), before any configuration loading or logging initialization. This prevents unnecessary node initialization overhead for client commands.
- **Async Execution**: Client command execution is async (requires tokio runtime). The main() function is already `#[tokio::main]`, so execute_command() can be awaited directly.
- **Error Handling Strategy**: Use `anyhow::Context` to provide rich error messages. Map RPC client errors to user-friendly formats (e.g., "Failed to connect to RPC server at http://localhost:10332: connection refused").
- **Backwards Compatibility**: All existing node daemon flags and behavior remain unchanged. Client commands are purely additive. Zero breaking changes to existing deployments.
- **Testing Constraint**: Integration tests should use a mock RPC server (wiremock crate) to avoid dependency on external Neo nodes. Unit tests should focus on CLI parsing and routing logic.
