# neo-validator

`neo-validator` is an independent online state validator and fail-closed JSON-RPC safety gateway for one neo-cli node and one neo-go node. It replays identical blocks through the real neo-rs execution and incremental MPT pipeline, compares execution receipts and state roots, and exposes an upstream RPC only at a fully verified watermark.

It isolates **RPC responses**, not P2P or consensus. The upstream nodes can continue producing and syncing blocks internally while their client-facing gateways are closed.

## Required deployment boundary

The gateway cannot protect a route that bypasses it. For each upstream:

1. Bind its original RPC listener to `127.0.0.1` (or an equivalently restricted interface).
2. Use host firewall rules to prevent every other machine and unprivileged local process from reaching that original port.
3. Point clients only to the corresponding `proxy_listen` port.
4. Enable StateService-compatible `getstateroot` and ApplicationLogs-compatible `getapplicationlog` on **both** nodes. Missing methods stop strict startup or quarantine validation; there is no hash-only fallback.
5. Give the validator read access only to node configuration files. It never opens neo-cli RocksDB or neo-go BoltDB storage.

The validator performs two `getblockcount` calls per proxied request, before and after forwarding. If the upstream is ahead of the verified watermark, advances during the request, changes gate generation, or becomes unavailable, the response is discarded and the client receives HTTP 503 with a JSON-RPC error. This closes the polling-gap and in-flight-response windows, at the cost of additional RPC traffic.

## Validation rules

Validation starts at genesis. The validator does not import an upstream's current state root as a fake local baseline. Both nodes must serve historical blocks from the validator's current height until they reach a common tip.

For every height, all of the following must hold:

- `getblock` Base64 decodes strictly within the byte limit, consumes the entire wire payload, and has the expected height and hash.
- neo-cli and neo-go return byte-identical blocks, hashes, execution receipts, and state roots.
- Execution receipt comparison covers trigger, final VM state, exception presence, and GAS consumed. Exception text is diagnostic and is not normalized across implementations.
- neo-rs really executes the block into its dedicated ledger and produces the same block hash, receipts, and incremental MPT root.
- Restart recovery re-reads the persisted checkpoint and MPT baseline and verifies them against both upstreams before opening either gateway.

Raw-block divergence, receipt/root divergence, malformed evidence, missing required RPC capabilities, and execution failures quarantine both gateways. A transport failure quarantines only the affected upstream; the other remains restricted to its existing verified watermark.

## Configure and run

Copy `neo-validator.example.toml`, set actual paths, network magic, complete protocol JSON, token digests, and the RPC method policy, then run:

```powershell
cargo run -p neo-validator -- --config .\neo-validator.toml --check-config
cargo run -p neo-validator --release -- --config .\neo-validator.toml
```

`protocol_path` may be omitted when discovery of the official neo-cli root `config.json` (`ProtocolConfiguration`) and `protocol.*.json` files finds exactly one distinct protocol configuration matching `network.magic`. Simplified protocol conversion is deliberately not used. `getversion` on both nodes must match the execution settings, including hardforks and standby committee. Both neo-cli's hardfork object and neo-go's `{name, blockheight}` array are supported; duplicate, unknown, missing, or mismatched entries are rejected.

## Verified interoperability

The strict pipeline has been exercised against official **neo-cli v3.10.2** and **neo-go v0.123.0** binaries on a temporary single-validator private network, not only against mock RPC servers. The validator replayed from genesis, followed newly produced blocks, passed the live proxy authorization/allowlist/lag/recovery checks, and reopened both gateways after a same-database restart. The reproducible local harness and JSON evidence are under `.t30-live/`; its tokens and wallet password are test-only and must not be reused.

The live run also exposed and led to fixes for two consensus/storage boundary issues: LedgerContract bookkeeping is excluded from the MPT contract-storage root (matching both upstream implementations), and RocksDB latest-state read-cache entries are invalidated only after successful atomic commits so a concurrent read cannot preserve a stale block pointer. These fixes have dedicated regression tests in addition to the live evidence.

Generate SHA-256 token digests without putting plaintext tokens in TOML:

```powershell
function Get-TokenDigest([string]$Token) {
  $bytes = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Token))
  [Convert]::ToHexString($bytes).ToLowerInvariant()
}
Get-TokenDigest $env:NEO_VALIDATOR_ADMIN_TOKEN
Get-TokenDigest $env:NEO_VALIDATOR_READ_TOKEN
```

Use long, randomly generated, different admin/read tokens. Protect the audit and data directories with filesystem ACLs. Upstream RPC usernames/passwords, when needed, come only from the environment variables named by `rpc_username_env` and `rpc_password_env`.

Remote upstreams require `allow_remote_upstreams = true` and HTTPS. Public proxy listeners require `allow_public_proxy = true` and `proxy_token_sha256`; deploy a controlled TLS terminator for non-loopback clients. The admin API is always loopback-only.

## Status and authenticated recovery

Read status with either the read or admin token:

```powershell
curl.exe -H "Authorization: Bearer $env:NEO_VALIDATOR_READ_TOKEN" http://127.0.0.1:20400/status
```

An incident remains recorded in the JSONL audit log. Recovery never deletes evidence, advances the chain, or changes a state root. After repairing the upstream cause, submit the active incident id and the **last verified** baseline:

```powershell
curl.exe -X POST http://127.0.0.1:20400/resume `
  -H "Authorization: Bearer $env:NEO_VALIDATOR_ADMIN_TOKEN" `
  -H "Content-Type: application/json" `
  --data '{"incident_id":"ACTIVE_ID","height":123,"block_hash":"0xVERIFIED_HASH","confirmation":"resume-at-verified-baseline"}'
```

Recovery is rejected if the local executor has already persisted beyond that baseline (for example, after a local state-root mismatch). In that case, preserve the incident evidence and rebuild/repair the dedicated validator ledger rather than clearing the error. Successful recovery returns to `verifying`; gateways reopen only after validation succeeds again.

## Operational limits

- Initial full replay can be time- and disk-intensive. Pruned historical blocks are incompatible with a new validator ledger.
- `max_catch_up_blocks` bounds work per polling cycle; it does not permit skipping evidence.
- Request/response/block limits, RPC timeouts, listener policy, and method allowlists are mandatory validated settings.
- The JSONL audit contains incidents and recovery actions, not RPC bodies, credentials, or tokens.
- Build without default features disables RocksDB and causes production executor startup to fail explicitly.
