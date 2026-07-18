## Context

Neo N3 nodes can independently bootstrap by downloading blocks, validating the
header and witness chain, and executing every transaction with the canonical
`neo-vm`. That remains the strongest path, but it repeats historical execution
and transfers the same archive from full-node peers to every new node.

StateService provides a quorum-signed `StateRoot` for the non-Ledger MPT. The
root is a strong checkpoint anchor when its witness is verified against an
independently trusted StateValidator set, but the StateService projection
deliberately excludes Ledger contract ID `-4`. A fast checkpoint must therefore
authenticate both the existing StateRoot and the exact Ledger data needed by
future execution. NeoFS can store and distribute the bytes, but it neither
selects the canonical Neo chain nor proves execution, freshness, completeness,
or continued availability.

The current FullState MPT is not an appropriate transport format. The measured
height-3,277,022 node namespace contains 218,355,320 historical node rows and a
completed persistence pack requires roughly 60.6 GiB of payload and live
indexes. V1 instead transports current canonical rows and reconstructs a
pruning-mode MPT locally, avoiding historical nodes and untrusted reference
counts.

## Goals / Non-Goals

**Goals:**

- Preserve full replay as an independently verified bootstrap mode while using
  NeoFS as an additional immutable block source.
- Define one canonical, deterministic checkpoint identity and certificate that
  can be reproduced by independent builders.
- Authenticate current non-Ledger state with the existing N3 signed StateRoot
  and authenticate Ledger/block bindings with the same independently anchored
  StateValidator trust assumption.
- Import a checkpoint without exposing partial state across crashes, disk-full
  failures, corrupt downloads, process termination, or restart.
- Continue canonical execution at height `H + 1` with byte-identical Ledger,
  MPT roots, VM outcomes, and protocol behavior.
- Let a light client verify current state, Ledger-row, block, and transaction
  point queries without trusting the gateway that serves them.
- Bound memory, disk, object, row, decompression, network, and retry resources.
- Keep checkpoint authority disabled by default until deterministic export,
  independent verification, MainNet continuation, corruption, and crash gates
  pass.

**Non-Goals:**

- Changing Neo N3 consensus, network messages, StateRoot, block, transaction,
  VM, MPT, native-contract, or canonical Ledger encodings.
- Proving all pre-checkpoint execution with ZK, SNARK, STARK, or recursive
  validity proofs.
- Transporting the FullState historical MPT or a compact serialized MPT.
- Serving arbitrary historical state roots, range proofs, prefix scans,
  application logs, or notification archives.
- Erasure coding, direct light-client traversal of NeoFS objects, background
  MPT construction, checkpoint deltas, or activation before the MPT is ready.
- Treating NeoFS object identity, search results, or availability as canonical
  chain selection or finality.

## Decisions

### 1. Keep replay and certified checkpoint modes explicit

`replay` downloads immutable block segments from NeoFS, HTTP, or P2P, verifies
the canonical header/block/transaction structure, and executes genesis through
the tip with `neo-vm`. NeoFS changes only the byte source.

`certified-checkpoint` downloads headers, current state, current Ledger rows,
and optionally the complete block archive. It verifies a locally anchored
StateValidator certificate, reconstructs the current MPT, activates height H,
and executes H+1 onward. RPC and metrics expose the selected mode and the
checkpoint identity. No mode silently falls back to weaker verification.

Alternative: call every checkpoint trustless because StateRoot is signed.
Rejected because StateRoot excludes Ledger and does not bind the canonical
block hash or archive commitment.

### 2. Use a canonical checkpoint core and detached attestations

`CheckpointCoreV1` is a bounded canonical binary record with fixed field order
and no transport locators. It contains:

```text
format_version and domain
network_magic and genesis_hash
height and canonical_block_hash
protocol_settings, hardfork, native-registry, neo-vm, MPT-codec digests
complete signed StateRoot wire bytes
trusted_state_validator_set_hash
non_ledger_stream_commitment
ledger_stream_commitment
header_archive_commitment
block_archive_commitment
row, value, block, chunk, and logical-byte counts
```

`checkpoint_id = SHA256("neo-checkpoint-v1\0" || core_bytes)`. A detached
`CheckpointCertificateV1` contains the checkpoint ID and a canonical Neo
multisignature witness over domain-separated sign data. The verifier derives
the BFT multisig script from the locally trusted StateValidator keys and uses
the canonical Neo witness engine. The checkpoint payload never supplies its
own trust authority.

The complete signed N3 StateRoot remains independently verified using its
existing sign data. Its index and root must equal the checkpoint fields. The
supplementary certificate does not replace or modify StateRoot; it binds the
Ledger and block commitments omitted from StateService.

Alternative: have the publisher sign a JSON manifest. Rejected because it adds
publisher trust, permits encoding ambiguity, and does not authenticate Ledger
under the StateValidator assumption.

### 3. Use a local weak-subjectivity trust anchor

Each network configuration supplies a `TrustedCheckpointAnchor` containing the
network/genesis identity, minimum accepted height, and independently obtained
StateValidator public keys or their exact hash. Existing nodes may advance from
their last locally verified anchor. Fresh nodes use an anchor distributed and
authenticated outside NeoFS. V1 rejects a certificate if its signer set differs
from the anchor; a validator rotation requires installing a new independently
authenticated anchor before accepting checkpoints signed by the new set.

This deliberately favors a small, auditable V1 over an automatic validator
rotation proof chain. The node persists the highest accepted height and ID,
rejects downgrade, and stops on two valid conflicting IDs at one height.

Alternative: read StateValidators from the downloaded checkpoint and verify
its signatures with those keys. Rejected as circular self-authentication.

### 4. Transport canonical flat rows, not serialized MPT nodes

One frozen coordinated read transaction exports two disjoint streams:

- `state`: every live raw `StorageKey -> StorageItem` row except contract ID
  `-4`, strictly ordered by raw key bytes with no duplicate keys;
- `ledger`: every live raw Ledger contract ID `-4` row, using the same ordering
  and exact bytes.

Rows use bounded length-prefixed canonical encoding without decode/re-encode.
The state stream is the sole input to a canonical pruning-mode Neo MPT rebuild.
The rebuilt root must equal the signed StateRoot. The Ledger stream is imported
verbatim and its stream commitment is authenticated by the checkpoint
certificate. MPT nodes and their reference counts are derived locally.

Alternative: transport the current reachable MPT for faster activation.
Rejected from V1 because it duplicates data, introduces graph/refcount
validation, and is only an acceleration artifact.

### 5. Use deterministic Merkle commitments and bounded chunks

Each stream commitment uses SHA-256 with distinct leaf and internal-node domain
bytes and an unambiguous empty root. A row leaf commits to stream kind, ordinal,
raw key length and bytes, raw value length and bytes. The tree uses a specified
largest-power-of-two split, so independent implementations produce the same
root for non-power-of-two row counts. Strict ordering plus row count and
neighbor proofs provide Ledger non-membership.

The logical stream is split at row boundaries into deterministic target-size
chunks. Each `ChunkDescriptorV1` commits to stream kind, ordinal, first/last
key, row count, logical length/hash, encoded length/hash, and compression ID.
V1 uses fixed deterministic zstd settings, a 32 MiB logical target, and a hard
48 MiB encoded-object limit. Importers enforce protocol-derived row/value
limits and declared aggregate limits before allocation or decompression.

The block product reuses canonical immutable header/block bytes and existing
static-file segment semantics. A deterministic segment catalog commits to
height ranges and logical hashes through H. Header-chain and block transaction
Merkle validation remain mandatory; archive hashes are not substitutes.

Alternative: package a ZIP/TAR database image. Rejected because it is
backend-specific, difficult to reproduce, unsafe to extract, and cannot prove
logical state completeness against StateRoot.

### 6. Separate logical identity from NeoFS location

A small `TransportManifestV1` maps checkpoint chunk hashes to one or more
NeoFS object IDs and fallback HTTP/P2P locators. It is not part of the
checkpoint ID and is never an authority: every downloaded object is accepted
only after descriptor, encoded hash, decoded hash, geometry, and stream-root
verification.

V1 uses two configured NeoFS containers:

- metadata container with replication factor five for core, certificate, and
  transport manifests;
- bulk container with replication factor three for state, Ledger, headers, and
  blocks.

Publishers upload all bulk objects, read back and verify them, upload the
transport manifest, and publish the core/certificate reference last. Clients
use direct object IDs instead of depending on incomplete searches. Missing or
withheld objects make that source unavailable; clients retry another locator
or use P2P/full replay. V1 does not use erasure coding.

### 7. Export only from one frozen, internally consistent height

The exporter acquires a checkpoint lock and opens one coordinated MDBX read
transaction spanning canonical and StateService namespaces. It verifies that
the Ledger tip, local MPT root, validated signed StateRoot, header tip, and
static archive all identify H before and after streaming. It writes immutable
chunks to a staging directory, publishes a deterministic core, and emits a
signing request.

Each StateValidator signer independently verifies network/protocol identity,
signed StateRoot, local height/root, stream commitments or a deterministic
local rebuild, and block/archive commitments before signing. A collector forms
the standard BFT quorum witness. It never asks validators to sign an arbitrary
publisher-provided hash without local validation.

Alternative: copy tables with separate read transactions. Rejected because a
commit between scans can produce a cross-height checkpoint whose individual
parts look valid.

### 8. Activate one complete generation atomically

Checkpoint import runs before the node opens its canonical MDBX environment.
V1 supports only a coordinated canonical/StateService environment and an
archive path owned by the same generation. Unsupported split-store layouts are
rejected.

The importer state machine is:

```text
DISCOVERED -> DOWNLOADING -> CONTENT_VERIFIED -> TRUST_VERIFIED
-> IMPORTING -> ROOT_VERIFIED -> READY -> ACTIVE
```

Downloads land in content-addressed `.part` files with resumable verified
ranges. Import builds `generations/<checkpoint-id>.staging`, bulk-loads exact
state and Ledger rows, installs verified header/block segments, rebuilds the
pruning-mode MPT, persists the signed StateRoot/certificate, and verifies root,
height, block hash, Ledger tip, row counts, stream roots, reopen state, and a
deterministic read digest.

After syncing all database/files and parent directories, the importer renames
the generation to its final name, writes and fsyncs `READY`, and atomically
replaces a small `CURRENT` pointer. This pointer is the only activation point.
Startup ignores incomplete staging generations. The previous ready generation
is retained until the new generation restarts and completes a configured
strict MainNet continuation window. Rollback occurs only while the node is
stopped and atomically changes `CURRENT`.

Alternative: use the existing fast-sync marker around live-table mutation.
Rejected because it does not provide a complete cross-file rollback boundary.

### 9. Verify light queries locally through an untrusted gateway

V1 light clients accept a checkpoint core only after verifying the local trust
anchor, signed StateRoot, checkpoint certificate, header anchor, format and
protocol IDs, and anti-rollback state. A proof gateway may use local node data
or NeoFS-backed indexes, but it is not trusted.

- Current non-Ledger state queries return the raw value or absence plus a
  canonical Neo MPT proof verified against the signed StateRoot.
- Ledger point queries return a row inclusion proof, or adjacent-boundary
  non-membership proof, verified against the certified sorted Ledger root.
- Transaction queries return the raw header/block/transaction and Merkle path
  verified against the accepted canonical block hash/header chain.

V1 exposes no arbitrary prefix/range proof, notification/application-log
proof, direct NeoFS MPT traversal, or historical state query. Gateway failures
are retried elsewhere and cannot change the locally verified result.

### 10. Fail closed with bounded resources and explicit evidence

Configuration bounds total logical/encoded bytes, chunk and row counts,
individual value size, decompression ratio, concurrent requests, retries,
timeouts, memory, open files, disk reserve, staging generations, and retained
ready generations. Import never follows archive paths or symlinks because V1
has no general archive extraction step.

Reports expose bounded-label download, verification, decompression, import,
MPT rebuild, fsync, proof, retry, and continuation timing; source-failure and
trust-failure reasons; logical/physical bytes; and NeoFS price parameters used
for a non-authoritative cost estimate. A promotion report names checkpoint ID,
network, height, corpus, hardware, filesystem, binary digest, trust anchor,
root, block hash, crash matrix, and strict continuation result.

## Risks / Trade-offs

- **[A malicious checkpoint supplies its own validator set]** -> Resolve keys
  only from the local trusted anchor and reject signer-set mismatch.
- **[StateRoot is mistaken for a full Ledger commitment]** -> Require the
  supplementary certificate before checkpoint activation and keep replay as
  the independent alternative.
- **[StateValidator quorum signs bad auxiliary data]** -> Require signers to
  validate from local canonical state; document that V1 inherits the same
  quorum trust assumption and does not prove history from genesis.
- **[StateValidators rotate]** -> Fail closed until an independently
  authenticated replacement anchor is installed; automatic rotation proofs
  remain outside V1.
- **[NeoFS serves stale, missing, or corrupt objects]** -> Verify content and
  anti-rollback state, use multiple direct locators, and fall back to P2P/replay.
- **[Flat-state rebuild is slower than downloading an MPT]** -> Use streaming
  bulk load and benchmark the canonical builder; accept this bounded one-time
  cost to avoid transporting and trusting derived MPT graph state in V1.
- **[Crash or ENOSPC exposes mixed heights]** -> Build an isolated generation,
  fsync every authority boundary, and activate only through `CURRENT`.
- **[Untrusted geometry exhausts resources]** -> Validate all counts and sizes
  before allocation and enforce aggregate, chunk, decompression, and disk caps.
- **[A light gateway fabricates a value or absence]** -> Require local MPT or
  sorted-Merkle proof verification; transport hashes alone are insufficient.
- **[A valid checkpoint has incomplete historical RPC coverage]** -> Expose
  the checkpoint history floor and reject unsupported historical queries rather
  than synthesize results.

## Migration Plan

1. Freeze canonical encodings and cross-language test vectors for core,
   certificate sign data, rows, Merkle trees, chunks, and transport manifest.
2. Add coordinated snapshot/export and offline verification without NeoFS or
   node activation; reproduce identical IDs from two independent exports.
3. Add StateValidator signer/collector tooling and verify certificates against
   locally pinned MainNet fixtures.
4. Add NeoFS publication/download in shadow tooling, with HTTP/P2P fallback,
   corruption, withholding, retry, and bounded-resource tests.
5. Add generation import in verify-only mode and compare complete MDBX/State
   dumps, roots, Ledger rows, headers, blocks, and reopen behavior with the
   source node.
6. Enable opt-in certified checkpoint activation, then execute strict MainNet
   continuation windows across hardfork and transaction-heavy boundaries.
7. Add the proof gateway/light verifier and cross-check every proof against the
   ordinary full-node RPC/state provider.
8. Keep the production default on replay until independent hosts pass the
   deterministic export, crash matrix, corruption, continuation, and rollback
   gates. Rollback removes unpublished staging data or atomically restores the
   previous ready generation.

## Open Questions

- Select the first checkpoint cadence and retention count from measured export,
  import, NeoFS retrieval, and recovery-drill cost; these are operational policy
  values, not format fields.
- Select the fixed zstd level after a CPU/bandwidth bakeoff while retaining the
  same uncompressed logical commitment and format compatibility.
