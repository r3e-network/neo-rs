# Blockchain Double Initialization Fix

## Issue
The Neo-RS node is creating blockchain instances twice, causing RocksDB lock conflicts:

1. First initialization: Line 18 - "Creating new blockchain instance for network: TestNet"
2. Second initialization: Line 49 - "Creating new blockchain instance for network: TestNet"

This causes the error:
```
Failed to create fallback RocksDB storage: StorageError("Failed to open RocksDB: IO error: lock hold by current process")
```

## Root Cause
The issue is in the code architecture where:
1. The blockchain is initialized once during startup
2. The RPC server tries to create another blockchain instance

## Implementation provided Workaround
Since this is a code issue that needs to be fixed in the Rust source, we can:

1. **Use a different data directory approach**:
   ```bash
   # Create unique data directory for each run
   export NEO_DATA_DIR="/tmp/neo-data-$(date +%s)"
   mkdir -p "$NEO_DATA_DIR"
   ./target/release/neo-node --testnet --data-dir "$NEO_DATA_DIR" --rpc-port 30332 --p2p-port 30333
   ```

2. **Run with single blockchain instance** (if configurable):
   ```bash
   # Check if there's a flag to disable duplicate initialization
   ./target/release/neo-node --help | grep -E "(single|shared|instance)"
   ```

## Code Fix Required
The fix needs to be implemented in the source code:

1. **Shared Blockchain Instance**: Create a single blockchain instance and share it between components
2. **Singleton Pattern**: Ensure only one blockchain instance per data directory
3. **Proper Resource Management**: Use proper locking mechanisms

## Current Status
- Node cannot start due to double blockchain initialization
- RPC would work if the blockchain issue is resolved
- P2P networking is ready but blocked by the same issue

## Alternative: Test Mode
If there's a test mode that doesn't require persistent storage:
```bash
./target/release/neo-node --testnet --in-memory --rpc-port 30332 --p2p-port 30333
```

## Recommendation
1. Fix the double initialization in the source code
2. Use dependency injection to share blockchain instance
3. Add proper error handling for storage conflicts