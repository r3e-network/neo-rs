# Parity Summary & Porting Queue

Total C# files analyzed (minus exclusions): 649
Matched Rust files: 158
Missing Rust files: 491

## Category Totals

| Category | Missing files |
| -------- | ------------- |
| Core stack | 220 |
| Plugin suite | 116 |
| Tooling & aux | 154 |

### Core stack breakdown

| Module | Missing files |
| ------ | ------------- |
| SmartContract | 61 |
| Network | 58 |
| VM | 38 |
| Core | 31 |
| Persistence | 14 |
| Wallets | 10 |
| Ledger | 8 |

### Plugin breakdown

| Plugin | Missing files |
| ------ | ------------- |
| RpcServer | 22 |
| DBFTPlugin | 20 |
| ApplicationLogs | 14 |
| LevelDBStore | 14 |
| TokensTracker | 12 |
| StateService | 10 |
| SQLiteWallet | 9 |
| OracleService | 6 |
| RocksDBStore | 4 |
| SignClient | 3 |
| StorageDumper | 2 |
| RestServer | 0 |

### Tooling breakdown

| Module | Missing files |
| ------ | ------------- |
| RpcClient | 35 |
| Extensions | 33 |
| BLS12_381 | 26 |
| CLI | 22 |
| JSON | 12 |
| MPTTrie | 9 |
| IO | 9 |
| ConsoleService | 8 |

## Porting Queue (Top gaps by group)

- Neo.VM: 38 missing files
- RpcClient: 35 missing files
- Neo/SmartContract: 29 missing files
- Neo/Network/P2P/Payloads: 28 missing files
- Neo.Cryptography.BLS12_381: 26 missing files
- Neo.CLI: 22 missing files
- Plugins/RpcServer: 22 missing files
- Plugins/DBFTPlugin: 20 missing files
- Neo/Network/P2P: 18 missing files
- Neo/Persistence: 14 missing files
- Neo/SmartContract/Native: 14 missing files
- Plugins/ApplicationLogs: 14 missing files
- Plugins/LevelDBStore: 14 missing files
- Neo.Json: 12 missing files
- Plugins/TokensTracker: 12 missing files

## Recommended Sequence

1. Restore external interfaces: focus on RpcClient (35) and RpcServer (22) now that RestServer scaffolding exists.
2. Complete consensus & networking: DBFTPlugin (20) plus Neo/Network payloads (28) and related helpers.
3. Port VM and smart-contract runtime gaps: Neo.VM (39) and the SmartContract family (59).
4. Implement persistence and wallet layers: Neo/Persistence (14) and Neo/Wallets (10) including storage plugins.
5. Backfill tooling/developer UX: Neo.CLI (22), ConsoleService (8), and Extensions/IO helpers (33).
