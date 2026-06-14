# JSON-RPC API Reference

The node exposes a JSON-RPC 2.0 API over HTTP. The endpoint listens on the
address and port configured in the `[rpc]` section of your TOML config
(default `127.0.0.1:10332`). All requests are `POST` to the root path `/`
with `Content-Type: application/json`. Parameters are passed positionally as a
JSON array.

This API targets parity with the C# Neo `RpcServer` plugin: every method name
and its parameter order match the reference node, so existing Neo tooling and
SDKs work unchanged.

```mermaid
flowchart LR
  Client["RPC client<br/>(curl / SDK / dApp)"] -->|HTTP POST JSON-RPC| Server["neo-rpc server<br/>(jsonrpsee)"]
  Server -->|disabled_methods filter| Dispatch["method dispatch"]
  Dispatch --> Blockchain["Blockchain &amp; Ledger"]
  Dispatch --> Contract["VM invocation"]
  Dispatch --> State["State &amp; MPT proofs"]
  Dispatch --> Wallet["Wallet (opened required)"]
  Dispatch --> Plugins["Plugin groups<br/>(logs, tokens, oracle)"]
```

## Request and response shape

A request is a JSON object with `jsonrpc`, `method`, `params`, and `id`:

```json
{ "jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1 }
```

A success response carries `result`; an error response carries an `error`
object with a numeric `code` and `message`. Unknown methods, malformed
parameters, and disabled methods all return JSON-RPC errors.

## Endpoint configuration

The endpoint is governed by the `[rpc]` config section. The node daemon wires
through only these three keys:

| Key | Shipped configs | Purpose |
|-----|-----------------|---------|
| `enabled` | `true` | Whether the RPC server starts (omitted ⇒ off) |
| `bind_address` | `127.0.0.1` | Listen address |
| `port` | `10332` (MainNet) / `20332` (TestNet) | Listen port |

The deeper RPC knobs (`rpc_user`/`rpc_pass`, `disabled_methods`, CORS, request
limits) live in the RPC server's own settings model and the server-enforced DoS
limits described in [operations.md](./operations.md); they are documented in
[configuration.md](./configuration.md), which is the authoritative config
reference. Notes on the supported surface:

- **TLS** is not terminated in-process; bind to localhost or place the node
  behind a TLS-terminating reverse proxy for remote access (see
  [operations.md](./operations.md)).
- **Wallet methods** require an opened wallet and are intended to be disabled on
  untrusted networks (`openwallet` is in `disabled_methods` in the shipped
  configs).
- **WebSocket subscriptions** are not enabled by the node daemon; the HTTP
  JSON-RPC endpoint above is the supported interface.

## curl examples

Get the node version:

```bash
curl -s http://127.0.0.1:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'
```

Get the current block height (count):

```bash
curl -s http://127.0.0.1:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'
```

Get a block by index, verbose form:

```bash
curl -s http://127.0.0.1:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getblock","params":[0, true],"id":1}'
```

---

## Blockchain

Read-only queries over blocks, transactions, contracts, storage, and committee
state. All names are C#-parity named.

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `getbestblockhash` | none | Hash of the latest block | |
| `getblockcount` | none | Block count (height + 1) | |
| `getblockheadercount` | none | Header count | |
| `getblockhash` | `index` | Block hash at height | |
| `getblock` | `hash`\|`index`, `verbose?` | Block (hex or JSON) | `verbose` boolean or `0`/`1` |
| `getblockheader` | `hash`\|`index`, `verbose?` | Header (hex or JSON) | |
| `getblocksysfee` | `index` | Cumulative system fee | |
| `getrawmempool` | `should_get_unverified?` | Pending tx hashes | |
| `getrawtransaction` | `txid`, `verbose?` | Transaction (hex or JSON) | |
| `gettransactionheight` | `txid` | Block index of a tx | |
| `getcontractstate` | `hash`\|`id`\|`name` | Contract manifest + state | Accepts script hash, native id, or native name |
| `getstorage` | `contract`, `key` (base64) | Stored value | |
| `findstorage` | `contract`, `prefix` (base64), `start?` | Paged storage entries | |
| `getnativecontracts` | none | All native contract states | |
| `getcommittee` | none | Committee public keys | |
| `getcandidates` | none | Registered candidates + votes | |
| `getnextblockvalidators` | none | Validators for the next block | |

## Smart contract invocation

Execute scripts against a read-only VM snapshot, verify witnesses, and drive
result iterators. These do not change chain state.

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `invokefunction` | `scripthash`, `method`, `params?`, `signers?` | VM result (state, gas, stack) | Optional signers/witnesses |
| `invokescript` | `script` (base64), `signers?` | VM result | |
| `invokecontractverify` | `scripthash`, `params?`, `signers?` | Witness verification result | |
| `getunclaimedgas` | `address` | Unclaimed GAS for an account | |
| `traverseiterator` | `session`, `iterator-id`, `count` | Next batch of iterator items | Session from a prior `invoke*` |
| `terminatesession` | `session` | Boolean | Releases an iterator session |

## State and MPT proofs

Merkle-Patricia state root, historical state lookups, and inclusion proofs.

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `getstateroot` | `index` | State root at a height | |
| `getstateheight` | none | Local + validated state heights | |
| `getstate` | `root-hash`, `contract`, `key` (base64) | Value at a historical root | |
| `findstates` | `root-hash`, `contract`, `prefix` (base64), `start?`, `count?` | Paged state entries with proofs | |
| `getproof` | `root-hash`, `contract`, `key` (base64) | Inclusion proof | |
| `verifyproof` | `root-hash`, `proof` (base64) | Verified value | Validates a proof from `getproof` |

## Node and network

Local node identity and peer connectivity; transaction and block submission.

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `getversion` | none | Protocol, port, and node info | |
| `getconnectioncount` | none | Number of connected peers | |
| `getpeers` | none | Connected / unconnected / bad peers | |
| `sendrawtransaction` | `tx` (base64) | Accepted tx hash | Relays to the mempool |
| `submitblock` | `block` (base64) | Submitted block hash | |

## Wallet

Wallet operations require an opened wallet on the node and are marked
**protected**: when RPC authentication is configured they require credentials.
`openwallet` is disabled by default (`disabled_methods`). These methods operate
on the wallet loaded into the running node, not a client-side wallet.

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `openwallet` | `path`, `password` | Boolean | Disabled by default in shipped configs |
| `closewallet` | none | Boolean | |
| `getnewaddress` | none | New address | |
| `listaddress` | none | Wallet addresses | |
| `dumpprivkey` | `address` | WIF private key | Sensitive |
| `importprivkey` | `wif` | Imported account | |
| `getwalletbalance` | `asset` | Asset balance | |
| `getwalletunclaimedgas` | none | Unclaimed GAS for the wallet | |
| `calculatenetworkfee` | `tx` (base64) | Network fee | |
| `sendfrom` | `asset`, `from`, `to`, `amount` | Signed/relayed tx | |
| `sendtoaddress` | `asset`, `to`, `amount` | Signed/relayed tx | |
| `sendmany` | `from?`, `outputs[]` | Signed/relayed tx | Multiple outputs |
| `canceltransaction` | `txid`, `signers[]`, `fee?` | Conflicting cancel tx | |

## Governance

Governance reads are served from the Blockchain group (`getcommittee`,
`getcandidates`, `getnextblockvalidators`, listed above) and the smart-contract
group (`getunclaimedgas`). There is no separate governance namespace; the names
match C# for tooling compatibility.

## Plugin method groups

The daemon registers three C#-optional plugin groups by default so the full RPC
surface is available out of the box.

### Application logs

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `getapplicationlog` | `txid`\|`blockhash`, `trigger?` | Execution log + notifications | |

### Token tracker (NEP-11 / NEP-17)

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `getnep17balances` | `address` | NEP-17 balances | |
| `getnep17transfers` | `address`, `start?`, `end?` | NEP-17 transfer history | |
| `getnep11balances` | `address` | NEP-11 balances | |
| `getnep11transfers` | `address`, `start?`, `end?` | NEP-11 transfer history | |
| `getnep11properties` | `contract`, `tokenid` | NEP-11 token properties | |

### Oracle

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `submitoracleresponse` | oracle response fields | Boolean | Used by oracle nodes |

## Utility

| Method | Parameters | Returns | Notes |
|--------|------------|---------|-------|
| `validateaddress` | `address` | Validity + script hash | |
| `listplugins` | none | Loaded plugins | |

## Error handling

Errors follow the JSON-RPC 2.0 error object with C#-parity codes. Common cases:

| Condition | Behaviour |
|-----------|-----------|
| Unknown method | Method-not-found error |
| Bad / missing parameters | Invalid-params error with a description |
| Method in `disabled_methods` | Error refusing the call |
| Wallet method without an open wallet | Error indicating no wallet is open |
| Auth configured, credentials missing/wrong | Request rejected before dispatch |
