## ADDED Requirements

### Requirement: Explicit replay and certified-checkpoint modes
The node SHALL expose distinct `replay` and `certified-checkpoint` bootstrap
modes, SHALL report the active mode and accepted checkpoint identity, and SHALL
not silently downgrade verification when any checkpoint prerequisite fails.

#### Scenario: Replay mode uses NeoFS blocks
- **WHEN** replay mode downloads a valid block segment from NeoFS instead of a peer
- **THEN** the node SHALL apply the ordinary header, witness, transaction, execution, Ledger, and StateRoot validation path without trusting the transport source

#### Scenario: Checkpoint verification fails
- **WHEN** any trust, content, root, Ledger, archive, reopen, or activation check fails in certified-checkpoint mode
- **THEN** the node SHALL reject and quarantine that checkpoint without opening a partially imported generation or falling back to an uncertified state

### Requirement: Deterministic checkpoint identity
The system SHALL encode `CheckpointCoreV1` canonically and SHALL derive its
checkpoint ID from a domain-separated SHA-256 commitment to the exact core
bytes. The core SHALL bind network and genesis identity, height and canonical
block hash, locally expected protocol identifiers, the complete signed N3
StateRoot, trusted StateValidator-set hash, state and Ledger stream
commitments, header and block archive commitments, and exact geometry counts.

#### Scenario: Independent builders export one frozen state
- **WHEN** two conforming builders export the same network, height, state, Ledger, signed StateRoot, headers, and blocks
- **THEN** their canonical core bytes, stream roots, and checkpoint IDs SHALL be byte-for-byte identical

#### Scenario: One committed field changes
- **WHEN** any committed byte, count, protocol identifier, height, block hash, StateRoot, stream root, or archive root differs
- **THEN** the checkpoint ID SHALL differ and an existing certificate SHALL not verify

#### Scenario: Transport location changes
- **WHEN** identical chunks are mirrored to different NeoFS objects or HTTP endpoints
- **THEN** the checkpoint ID SHALL remain unchanged because transport locators are excluded from the canonical core

### Requirement: Independently anchored StateValidator trust
The node SHALL resolve checkpoint verification keys only from a locally
authenticated `TrustedCheckpointAnchor`, SHALL verify the complete N3
StateRoot witness using the canonical Neo witness engine, and SHALL verify a
domain-separated supplementary checkpoint certificate from the required BFT
quorum over the checkpoint ID.

#### Scenario: Checkpoint supplies its own validators
- **WHEN** a payload contains validator keys or a witness that do not match the local trust anchor
- **THEN** verification SHALL fail even if those supplied keys form a valid self-signature

#### Scenario: StateRoot is valid but checkpoint certificate is absent
- **WHEN** the signed StateRoot witness is valid but no anchored quorum certificate binds the Ledger and block commitments
- **THEN** certified-checkpoint activation SHALL fail while replay mode remains available

#### Scenario: Validator set rotates
- **WHEN** a checkpoint certificate uses a set different from the installed trust anchor
- **THEN** the node SHALL fail closed until an independently authenticated replacement anchor is installed

### Requirement: Validators verify before signing
A checkpoint signer SHALL verify network and protocol identity, local canonical
height and block hash, signed StateRoot, deterministic stream commitments, and
header/block archive commitments from its own trusted data before contributing
a checkpoint signature. A collector SHALL form a certificate only from unique
valid signatures satisfying the canonical BFT quorum.

#### Scenario: Publisher proposes an arbitrary checkpoint ID
- **WHEN** the proposed core does not reproduce from a signer's local canonical snapshot
- **THEN** the signer SHALL refuse to sign and SHALL report the first mismatching committed field

#### Scenario: Collector receives duplicate or invalid signatures
- **WHEN** signatures are duplicated, malformed, from non-anchored keys, or below quorum
- **THEN** the collector SHALL reject the certificate rather than count or repair those signatures

### Requirement: Frozen canonical state export
The exporter SHALL read canonical and StateService namespaces from one frozen
coordinated snapshot and SHALL prove that Ledger tip, local MPT root, validated
signed StateRoot, header tip, and block archive all identify the same height
before and after export.

#### Scenario: Source advances during export
- **WHEN** any source height, root, block hash, or archive horizon differs from the frozen identity
- **THEN** export SHALL fail without publishing a checkpoint core or signing request

#### Scenario: Separate inconsistent snapshots are offered
- **WHEN** the storage backend cannot provide one coordinated frozen view of canonical and StateService data
- **THEN** the V1 exporter SHALL reject that layout instead of combining independently timed scans

### Requirement: Exact disjoint flat-state streams
The exporter SHALL emit exact raw `StorageKey -> StorageItem` bytes in strictly
increasing raw-key order with no duplicate keys. The state stream SHALL contain
all and only live non-Ledger rows; the Ledger stream SHALL contain all and only
live contract ID `-4` rows. Import SHALL reject any missing, extra, duplicated,
misclassified, reordered, or re-encoded row.

#### Scenario: Canonical rows are exported
- **WHEN** a frozen canonical snapshot is streamed
- **THEN** every live row SHALL appear exactly once in the correct stream with byte-identical key and value data

#### Scenario: A row is omitted or duplicated
- **WHEN** a chunk set omits one row, repeats one key, or places one Ledger row in the state stream
- **THEN** stream geometry or commitment verification SHALL fail before activation

#### Scenario: State rows rebuild the MPT
- **WHEN** all state-stream rows are inserted by the canonical pruning-mode MPT builder
- **THEN** the resulting root SHALL equal the root in the independently verified signed StateRoot

### Requirement: Deterministic bounded chunks
The system SHALL divide logical streams at row boundaries into deterministic
bounded chunks, SHALL commit encoded and decoded lengths and hashes, key range,
ordinal, row count, and compression identity, and SHALL use only the V1 fixed
compression profile. All geometry SHALL be validated before allocation or
decompression.

#### Scenario: A complete chunk is decoded
- **WHEN** encoded and decoded hashes, lengths, row bounds, ordering, and compression profile all match the descriptor
- **THEN** the importer SHALL expose exactly the declared logical records to stream verification

#### Scenario: Encoded input expands beyond its declaration
- **WHEN** a chunk exceeds its encoded limit, decoded limit, ratio limit, row limit, or aggregate checkpoint budget
- **THEN** the importer SHALL stop decoding, delete or quarantine the partial object, and fail without mutating an active generation

#### Scenario: Chunks are missing, reordered, or overlap
- **WHEN** chunk ordinals or key ranges contain a gap, duplicate, overlap, or out-of-order boundary
- **THEN** the stream SHALL fail completeness verification even if each available object has a valid object checksum

### Requirement: Verified immutable block archive
The block product SHALL contain deterministic contiguous header and block
segments through the checkpoint height. Consumers SHALL verify genesis and
parent linkage, consensus witnesses, block hashes, transaction Merkle roots,
transaction bytes, segment ranges, and the checkpoint block hash independently
of archive and NeoFS object hashes.

#### Scenario: Full replay consumes an archive
- **WHEN** all archive segments are present and structurally valid
- **THEN** replay SHALL execute them through the ordinary canonical import path and obtain the same Ledger, VM outcomes, and StateRoots as peer synchronization

#### Scenario: Archive object is internally hashed but contains a wrong block
- **WHEN** an object matches its transport descriptor but its header, block, transaction, or chain relation is invalid
- **THEN** canonical validation SHALL reject the archive and SHALL identify the first invalid height

### Requirement: NeoFS is replaceable untrusted transport
V1 SHALL publish metadata to a configured high-replication NeoFS container and
bulk chunks to a configured replicated bulk container. Publishers SHALL upload
and read-verify all referenced chunks before publishing the final descriptor
reference. Clients SHALL use direct locators, verify logical content, support
resumable downloads, and retry configured NeoFS, HTTP, or P2P alternatives.

#### Scenario: NeoFS returns corrupt bytes
- **WHEN** an object ID resolves but encoded or logical content differs from its descriptor
- **THEN** the client SHALL reject that source, retain no trusted result from it, and MAY retry an alternate locator

#### Scenario: NeoFS withholds a valid chunk
- **WHEN** a referenced object is unavailable after bounded retries
- **THEN** the checkpoint SHALL be reported unavailable rather than invalid, and the node SHALL remain able to use another source or full replay

#### Scenario: Descriptor is published before its chunks
- **WHEN** publisher read-back cannot retrieve and verify every referenced object
- **THEN** the publisher SHALL not publish the final checkpoint reference

### Requirement: Staged generation import and atomic activation
Certified-checkpoint import SHALL run before opening live node storage, SHALL
build every canonical, StateService, and archive component in one isolated
generation, and SHALL make that generation visible only through one fsynced
atomic `CURRENT` pointer replacement after all verification and reopen checks
pass.

#### Scenario: Process terminates during import
- **WHEN** the process stops at any download, write, sync, READY, rename, or pointer-update boundary
- **THEN** restart SHALL expose either the complete previous generation or the complete new generation, never a mixed or partially imported height

#### Scenario: Disk becomes full
- **WHEN** any staged write or durability operation returns `ENOSPC`
- **THEN** activation SHALL not occur and the existing current generation SHALL remain unchanged and restartable

#### Scenario: New generation is activated
- **WHEN** content, trust, MPT root, Ledger tip, block tip, stream roots, geometry, deterministic read digest, and reopen verification all pass and required files and directories are durable
- **THEN** the importer SHALL mark the generation READY and atomically replace CURRENT as the sole activation action

### Requirement: Exact continuation and rollback gate
An activated checkpoint generation SHALL execute H+1 onward through the
ordinary sequential authoritative pipeline. It SHALL remain provisional until
a configured strict MainNet continuation window matches expected block hashes,
VM artifacts, Ledger rows, MPT roots, signed StateRoots, and reopen state. The
previous ready generation SHALL remain available until that gate passes.

#### Scenario: Continuation matches
- **WHEN** the strict continuation window and restart verification complete without mismatch
- **THEN** the generation MAY become the retained current checkpoint baseline

#### Scenario: First continuation mismatch occurs
- **WHEN** any block, transaction result, Ledger value, MPT root, StateRoot, or artifact differs
- **THEN** the node SHALL stop, preserve first-divergence evidence, quarantine the checkpoint generation, and allow an offline atomic rollback to the previous ready generation

### Requirement: Anti-rollback and equivocation handling
The node SHALL durably record the highest accepted checkpoint height and ID,
SHALL reject lower checkpoints unless an explicit offline recovery policy
authorizes them, and SHALL stop on two otherwise valid conflicting checkpoint
IDs at one network and height.

#### Scenario: Stale catalog is served
- **WHEN** a source offers a valid checkpoint below the persisted acceptance floor
- **THEN** automatic checkpoint activation SHALL reject it while ordinary forward block synchronization remains available

#### Scenario: Quorum certificates conflict at one height
- **WHEN** two different checkpoint IDs at the same network and height both pass quorum verification
- **THEN** the node SHALL report equivocation, retain both artifacts, and SHALL not choose last-write-wins

### Requirement: Locally verifiable light point queries
A light client SHALL verify its checkpoint anchor and anti-rollback state
locally. An untrusted proof gateway SHALL support current non-Ledger state
membership and non-membership against the signed N3 StateRoot, Ledger-row
membership and sorted-boundary non-membership against the certified Ledger
root, and transaction inclusion against a verified block/header chain.

#### Scenario: State membership is returned
- **WHEN** a gateway returns a state value and canonical Neo MPT proof
- **THEN** the light client SHALL accept the value only if the proof verifies against the accepted signed StateRoot

#### Scenario: State absence is returned
- **WHEN** a gateway claims a key is absent
- **THEN** the light client SHALL accept absence only if a canonical non-membership proof verifies against the accepted signed StateRoot

#### Scenario: Ledger absence is returned
- **WHEN** a gateway claims a Ledger key is absent
- **THEN** the light client SHALL require valid adjacent sorted-row boundary proofs, or a valid edge proof, against the certified Ledger root

#### Scenario: Unsupported query is requested
- **WHEN** a client requests arbitrary range, prefix, historical-state, notification, or application-log proof behavior outside V1
- **THEN** the gateway SHALL return an explicit unsupported response rather than an unverified result

### Requirement: Bounded hostile-input handling
The downloader, verifier, importer, signer, gateway, and light verifier SHALL
enforce configured hard bounds for objects, chunks, rows, values, encoded and
decoded bytes, compression ratio, memory, file descriptors, disk reserve,
concurrency, requests, retries, and time. V1 SHALL not invoke a general archive
extractor or follow payload-provided paths or symlinks.

#### Scenario: Manifest declares excessive geometry
- **WHEN** any declared count or size exceeds the local policy or overflows arithmetic
- **THEN** validation SHALL fail before resource allocation, file creation, or database mutation

#### Scenario: Malformed record uses noncanonical length encoding
- **WHEN** a record has an overflowing, overlong, truncated, or noncanonical length field or trailing bytes
- **THEN** decoding SHALL fail closed and SHALL not expose a partial row

### Requirement: Observable promotion evidence
The system SHALL emit bounded-label metrics and machine-readable reports that
separate download, retry, hash, decompression, trust verification, flat import,
MPT rebuild, archive verification, fsync, reopen, proof, and continuation time
and bytes. Checkpoint authority SHALL remain disabled by default until declared
determinism, corruption, crash, rollback, MainNet parity, and continuation
gates pass on named hardware and filesystems.

#### Scenario: Promotion campaign completes
- **WHEN** an implementation is evaluated for opt-in activation
- **THEN** its report SHALL identify binary and checkpoint digests, network and height, trust anchor, hardware, filesystem, source locators, roots, first-divergence status, crash matrix, resource peaks, and strict continuation result

#### Scenario: Only a synthetic or partial test passes
- **WHEN** evidence covers only fixtures, a sampled namespace, an empty-block window, or a non-durable filesystem
- **THEN** it SHALL not satisfy the MainNet production-promotion gate
