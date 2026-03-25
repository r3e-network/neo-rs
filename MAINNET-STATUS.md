# Neo-RS Mainnet Node Status

## Deployment Information

- **Server**: 89.167.120.122
- **Deployed**: 2026-03-23 16:37 UTC
- **Status**: ✅ Running
- **Network**: Neo N3 Mainnet (magic: 860833102)

## Current Status

### Node Health

- **Process**: Running (PID verified)
- **Uptime**: Continuous since deployment
- **Memory**: 1.7GB / 30GB (5.7%)
- **Disk**: 179GB / 601GB (31%)

### Network Status

- **P2P Connections**: 10 active peers
- **RPC Port**: 10332 (operational)
- **P2P Port**: 10333 (listening)

### Sync Progress

- **Current Height**: 21,373
- **Target Height**: ~9,073,212 (mainnet)
- **Sync Status**: Active (early blocks)
- **Sync Speed**: ~12,000 blocks in first 2 minutes

## Validation Results

### ✅ Block Validation

- Block #1000 hash: `0xe31ad93809a2ac112b066e50a72ad4883cf9f94a155a7dea2f05e69417b2b9aa`
- Status: **VERIFIED** (matches expected hash)

### ✅ RPC Interface

- `getblockcount`: Working
- `getblock`: Working
- `getconnectioncount`: Working
- `getbestblockhash`: Working

### ✅ P2P Network

- Successfully connected to seed nodes
- Receiving block headers from peers
- Peer heights: 9,073,204 - 9,073,212

## Monitoring

### Continuous Validation

- Script: `/home/neo/git/neo-rs/scripts/validate-mainnet.sh`
- Monitor: `/home/neo/git/neo-rs/scripts/monitor-loop.sh` (running)
- Log: `/tmp/neo-monitor.log`

### Latest Check (2026-03-24 00:55 CST)

```
Block: 21373 | Connections: 10
```

## Next Steps

1. ⏳ Continue syncing to mainnet height (~9M blocks)
2. ⏳ Validate transaction execution once sync reaches blocks with transactions
3. ⏳ Compare state roots with C# reference node
4. ⏳ Run extended protocol compliance tests

## Commands

### Quick Status

```bash
ssh -i ~/.ssh/id_ed25519 root@89.167.120.122 \
  "curl -s --compressed -X POST http://localhost:10332 \
  -H 'Content-Type: application/json' \
  -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}' | jq"
```

### Full Validation

```bash
/home/neo/git/neo-rs/scripts/validate-mainnet.sh
```

### Monitor Logs

```bash
tail -f /tmp/neo-monitor.log
```
