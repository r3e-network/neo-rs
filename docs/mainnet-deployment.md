# Neo-RS Mainnet Deployment Guide

## Server Information

- **Host**: 89.167.120.122
- **OS**: Ubuntu 24.04 (Linux 6.8.0-90-generic)
- **RAM**: 30GB
- **Disk**: 402GB available
- **Access**: SSH with key `~/.ssh/id_ed25519`

## Deployment Steps

### 1. Install Dependencies

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# Install build tools
apt-get update
apt-get install -y build-essential pkg-config libssl-dev llvm-dev libclang-dev clang
```

### 2. Build Neo-RS

```bash
cd /root/neo-rs
cargo build --release
```

Build time: ~9 minutes 26 seconds
Binary size: 24MB at `/root/neo-rs/target/release/neo-node`

### 3. Configuration

Create `/root/neo-rs/neo_mainnet.toml`:

```toml
[network]
network_magic = 860833102

[storage]
backend = "rocksdb"
path = "/root/neo-data/mainnet"

[p2p]
listen_port = 10333
seed_nodes = [
  "seed1.neo.org:10333",
  "seed2.neo.org:10333",
  "seed3.neo.org:10333",
  "seed4.neo.org:10333",
  "seed5.neo.org:10333"
]

[rpc]
enabled = true
bind_address = "0.0.0.0"
port = 10332
```

### 4. Start Node

```bash
mkdir -p /root/neo-data/mainnet
cd /root/neo-rs
nohup ./target/release/neo-node --config neo_mainnet.toml > /root/neo-node.log 2>&1 &
```

## Validation

### Check Node Status

```bash
# Check process
pgrep -f neo-node

# Check logs
tail -f /root/neo-node.log

# Check block count
curl -s --compressed -X POST http://localhost:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' | jq .

# Check connections
curl -s --compressed -X POST http://localhost:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getconnectioncount","params":[],"id":1}' | jq .
```

### Verify Block Data

```bash
# Get block 1000
curl -s --compressed -X POST http://localhost:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblock","params":[1000, 1],"id":1}' | jq .
```

Expected hash for block #1000:

```
0xe31ad93809a2ac112b066e50a72ad4883cf9f94a155a7dea2f05e69417b2b9aa
```

## Deployment Results

✅ **Compilation**: Success (9m 26s)
✅ **Node Start**: Success
✅ **P2P Network**: 7 active connections
✅ **RPC Interface**: Operational on port 10332
✅ **Block Sync**: Active (21,373+ blocks synced)
✅ **Block Validation**: Hash verified for block #1000

## Monitoring

### Real-time Sync Progress

```bash
watch -n 5 'curl -s --compressed -X POST http://localhost:10332 \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}" | jq'
```

### Log Monitoring

```bash
tail -f /root/neo-node.log | grep -E "INFO|WARN|ERROR"
```

## Node Management

### Stop Node

```bash
pkill -f neo-node
```

### Restart Node

```bash
pkill -f neo-node
cd /root/neo-rs
nohup ./target/release/neo-node --config neo_mainnet.toml > /root/neo-node.log 2>&1 &
```

## Performance Notes

- Initial sync speed: ~12,000 blocks in first 2 minutes
- Memory usage: Stable under 30GB RAM
- Disk I/O: RocksDB backend performing well
- Network: 7+ peer connections maintained

## Next Steps

1. Monitor sync to current mainnet height (~9,073,000+ blocks)
2. Validate transaction execution against C# reference node
3. Compare state roots at key block heights
4. Run extended validation tests
