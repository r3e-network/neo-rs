# Neo CLI

Command-line client for Neo N3 blockchain nodes.

## Overview

`neo-cli` is a lightweight command-line tool that communicates with a Neo node
via JSON-RPC. It can connect to any Neo N3 node running the RpcServer plugin,
including `neo-node` or the official C# Neo-CLI.

## Installation

```bash
cargo build -p neo-cli --release
```

## Usage

```bash
# Basic usage (connects to localhost:10332)
neo-cli state

# Connect to a different node
neo-cli --rpc-url http://seed1.neo.org:10332 state

# With authentication
neo-cli --rpc-url http://localhost:10332 --rpc-user myuser --rpc-pass mypass state
```

## Commands

### Node Information

```bash
# Get node version
neo-cli version

# Show node state (height, best hash)
neo-cli state

# Show connected peers
neo-cli peers

# Show memory pool
neo-cli mempool
neo-cli mempool --verbose  # Include transaction hashes

# List loaded plugins
neo-cli plugins
```

### Blockchain Queries

```bash
# Get block by index
neo-cli block 1000

# Get block by hash
neo-cli block 0x1234...

# Get raw block hex
neo-cli block 1000 --raw

# Get block header
neo-cli header 1000

# Get transaction
neo-cli tx 0xabc123...

# Get contract state
neo-cli contract 0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5  # NEO token

# Get best block hash
neo-cli best-block-hash

# Get block count
neo-cli block-count

# Get block hash by index
neo-cli block-hash 1000
```

### Token Operations

```bash
# Get NEP-17 token balances
neo-cli balance NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq

# Get transfer history
neo-cli transfers NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq
neo-cli transfers NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq --from 1700000000 --to 1700100000
```

### Contract Invocation (Read-only)

```bash
# Invoke a contract method
neo-cli invoke 0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5 symbol '[]'

# With parameters
neo-cli invoke 0xd2a4cff31913016155e38e474a2c06d08be276cf balanceOf '[{"type":"Hash160","value":"0xabc..."}]'
```

### Utilities

```bash
# Validate an address
neo-cli validate-address NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq
```

## Global Options

| Option | Short | Description | Default |
|--------|-------|-------------|--------|
| `--rpc-url <URL>` | `-u` | RPC server URL | `http://localhost:10332` |
| `--rpc-user <USER>` | | RPC basic auth username | (none) |
| `--rpc-pass <PASS>` | | RPC basic auth password | (none) |
| `--output <FORMAT>` | `-o` | Output format (json, table, plain) | `plain` |
| `--help` | `-h` | Show help | |
| `--version` | `-V` | Show version | |

## Output Formats

```bash
# Plain text (default)
neo-cli state

# JSON output
neo-cli --output json state

# Table output (for list-like data)
neo-cli --output table peers
```

## Examples

### Check if Node is Running

```bash
$ neo-cli state
Block Height: 5234567
Header Height: 5234568
Best Block Hash: 0x1234567890abcdef...
```

### Query Token Balance

```bash
$ neo-cli balance NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq
{
  "address": "NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq",
  "balance": [
    {
      "assethash": "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5",
      "amount": "1000000000"
    }
  ]
}
```

### Query Block Information

```bash
$ neo-cli block 0
{
  "hash": "0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15",
  "size": 114,
  "version": 0,
  "previousblockhash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "merkleroot": "0x0000000000000000000000000000000000000000000000000000000000000000",
  ...
}
```

## Architecture

```
┌─────────────┐         JSON-RPC          ┌─────────────┐
│   neo-cli   │ ───────────────────────▶  │  neo-node   │
│  (client)   │      HTTP/HTTPS           │  (daemon)   │
└─────────────┘                           └─────────────┘
       │                                         │
       ▼                                         ▼
┌─────────────┐                           ┌─────────────┐
│ rpc_client  │                           │  NeoSystem  │
│   crate     │                           │  + Plugins  │
└─────────────┘                           └─────────────┘
```

## License

MIT License - see LICENSE file in the repository root.
