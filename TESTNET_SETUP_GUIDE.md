# Neo Rust TestNet Setup Guide

## Quick Start

### Option 1: Docker (Recommended)

```bash
# Build and run with Docker Compose
docker-compose -f testnet-docker-compose.yml up -d

# Monitor logs
docker-compose -f testnet-docker-compose.yml logs -f neo-testnet

# Verify functionality
./scripts/verify_testnet_functionality.sh
```

### Option 2: Native Build

```bash
# Install dependencies (Ubuntu/Debian)
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev librocksdb-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Build the node
cargo build --release --bin neo-node

# Run TestNet verification
./scripts/run_testnet_verification.sh --clean
```

## Verification Checklist

When the node is running, the verification script will test:

- ✅ **RPC Connectivity** - Can communicate with the node
- ✅ **P2P Network** - Connected to TestNet peers
- ✅ **Block Synchronization** - Receiving and processing new blocks
- ✅ **Transaction Handling** - Can retrieve and validate transactions
- ✅ **State Access** - Can read blockchain state
- ✅ **VM Execution** - Can execute smart contract calls
- ✅ **State Updates** - State is being updated correctly
- ✅ **Native Contracts** - All native contracts are accessible

## Expected Results

### Successful TestNet Connection

```
=== Test 1: RPC Connectivity ===
✓ PASS: RPC is accessible (Version: Neo-Rust/0.3.0)

=== Test 2: P2P Network Connectivity ===
✓ PASS: Connected to P2P network (8 peers)
Connected peers:
  - seed1t5.neo.org:20333 (neo-cli/3.6.0)
  - seed2t5.neo.org:20333 (neo-cli/3.6.0)
  - 192.168.1.100:20333 (neo-go/0.105.0)

=== Test 3: Block Synchronization ===
✓ PASS: Block sync working (synced 15 blocks in 30s)
Latest block:
  Height: 2847392
  Hash: 0x1234567890abcdef...
  Time: 1704723456
  Transactions: 3

=== Test 4: Transaction Handling ===
✓ PASS: Can retrieve and process transactions
Sample transaction:
  Hash: 0xabcdef1234567890...
  Type: ContractTransaction
  Size: 234 bytes
  System Fee: 997770

=== Test 5: State Access ===
✓ PASS: Can access contract state
NEO Contract:
  Name: NEO
  Hash: 0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5

=== Test 6: VM Execution ===
✓ PASS: VM execution successful
Invocation result:
  State: HALT
  GAS consumed: 0.0103
  Result: NEO

=== Test 7: State Updates ===
✓ PASS: State updates working (advanced 12 blocks)

=== Test 8: Native Contracts ===
✓ PASS: All native contracts accessible (5/5)

=== Test 9: Performance ===
✓ PASS: RPC response time: 45ms (excellent)

=== TestNet Verification Summary ===
Total Tests: 9
Passed: 9
Failed: 0
Success Rate: 100%

✓ All tests passed! Node is fully functional.
```

## Monitoring Commands

### Check Node Status
```bash
# Block height
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

# Peer connections
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getpeers","params":[],"id":1}'

# Node version
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'
```

### Health Monitoring
```bash
# Simple health check
curl http://localhost:8080/health

# Detailed health check
curl http://localhost:8080/health/detailed

# Continuous monitoring
./scripts/testnet_sync_monitor.sh
```

## Key Functionality Demonstrated

### 1. Block Synchronization ✅
- **Expected**: Node downloads blocks from TestNet peers
- **Verification**: Block height increases over time
- **Command**: `getblockcount` shows increasing height

### 2. P2P Network Connectivity ✅
- **Expected**: Connected to multiple TestNet seed nodes
- **Verification**: `getpeers` shows active connections
- **Performance**: Should maintain 5+ peer connections

### 3. Transaction Processing ✅
- **Expected**: Can retrieve and validate transactions
- **Verification**: `getrawtransaction` returns transaction details
- **State**: Mempool processes incoming transactions

### 4. VM Execution ✅
- **Expected**: Can execute smart contract methods
- **Verification**: `invokefunction` returns HALT state
- **Examples**: Native contract method calls (NEO.symbol())

### 5. State Management ✅
- **Expected**: Blockchain state is maintained correctly
- **Verification**: State height advances with blocks
- **Access**: Can read contract states and account balances

## Troubleshooting

### No Peer Connections
```bash
# Check firewall
sudo ufw allow 20333/tcp

# Test seed connectivity
nc -zv seed1t5.neo.org 20333
```

### Slow Sync
```bash
# Check resource usage
htop

# Monitor sync rate
watch -n 5 'curl -s http://localhost:20332 -d "{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}" | jq .result'
```

### RPC Not Responding
```bash
# Check if node is running
ps aux | grep neo-node

# Check logs
tail -f testnet-verification.log
```

## Performance Expectations

| Metric | Expected Range | Notes |
|--------|----------------|-------|
| Sync Speed | 50-200 blocks/sec | Depends on network conditions |
| Peer Count | 5-50 peers | TestNet has fewer peers than MainNet |
| RPC Response | < 100ms | For simple queries |
| Memory Usage | 2-4GB | During initial sync |
| Disk Space | 50-100GB | Current TestNet size |

## Next Steps

After successful TestNet verification:

1. **Monitor Stability** - Run for 24-48 hours
2. **Performance Testing** - Use network chaos testing
3. **Transaction Testing** - Deploy test contracts
4. **Validator Setup** - Configure consensus (if applicable)
5. **MainNet Preparation** - Security audit and optimization

## Support

- **Documentation**: See `docs/` directory
- **Monitoring**: Check `scripts/` for additional tools
- **Issues**: Review logs in TestNet verification output