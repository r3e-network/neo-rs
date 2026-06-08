# Testnet Validation Guide

## Prerequisites

1. Build release binary:

```bash
cargo build --release
```

2. Prepare testnet data directory:

```bash
mkdir -p ./data/testnet
```

## Running the Node

### Start Testnet Node

```bash
./target/release/neo-node --network testnet --data-dir ./data/testnet
```

### Configuration

Create `config.testnet.json`:

```json
{
    "network": "testnet",
    "rpc": {
        "enabled": true,
        "port": 20332
    },
    "p2p": {
        "port": 20333,
        "seeds": ["seed1t5.neo.org:20333", "seed2t5.neo.org:20333", "seed3t5.neo.org:20333"]
    }
}
```

Run with config:

```bash
./target/release/neo-node --config config.testnet.json
```

## Validation Steps

### 1. Block Sync Validation

Check sync progress:

```bash
curl -X POST http://localhost:20332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'
```

Compare with C# node:

```bash
# C# node
curl -X POST http://neo-testnet-node:20332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'
```

### 2. Block Hash Validation

Get block by height:

```bash
curl -X POST http://localhost:20332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblock","params":[1000, 1],"id":1}'
```

Verify hash matches C# node.

### 3. Transaction Execution Validation

Get application log:
```bash
curl -X POST http://localhost:20332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getapplicationlog","params":["<txid>"],"id":1}'
```

Compare execution result with C# node.

### 4. State Root Validation

```bash
curl -X POST http://localhost:20332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getstateroot","params":[1000],"id":1}'
```

### 5. Storage Validation

```bash
curl -X POST http://localhost:20332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getstorage","params":["<contract>","<key>"],"id":1}'
```

## Quick Start

```bash
# Build and run
cargo build --release
./target/release/neo-node --network testnet --data-dir ./data/testnet

# Monitor sync
watch -n 5 'curl -s -X POST http://localhost:20332 -H "Content-Type: application/json" -d "{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}" | jq'
```
