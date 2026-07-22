## ADDED Requirements

### Requirement: One immutable chain specification
The node SHALL select and validate one immutable `NeoChainSpec` before service
composition. The specification SHALL own the chain identity, protocol settings,
genesis inputs, and ordered hardfork schedule, and chain-aware composition
surfaces SHALL retain the selected specification through a shared
`Arc<NeoChainSpec>` rather than reconstructing protocol settings.

#### Scenario: Public-chain startup selects a coherent specification
- **WHEN** the application selects a built-in MainNet or TestNet specification
- **THEN** construction SHALL validate the network magic, address version, genesis configuration, validator and committee configuration, and hardfork schedule
- **AND** node composition SHALL receive the resulting `Arc<NeoChainSpec>`

#### Scenario: A chain-aware service exposes its selected specification
- **WHEN** a consumer requests chain rules through `ChainSpecProvider`
- **THEN** the provider SHALL return the shared immutable specification selected during composition
- **AND** the service SHALL derive protocol settings and hardfork activation from that specification

#### Scenario: A private network is selected
- **WHEN** the application selects a private network without supplying a complete specification
- **THEN** chain-spec construction SHALL reject the incomplete selection

### Requirement: Required node composition
The composition layer SHALL require every capability needed by the core node and
final node at builder construction time. Required storage, chain, blockchain,
network, transaction-pool, header-cache, native-contract-provider, and commit-hook
capabilities SHALL NOT be represented as optional fields that can fail only after
the node has been built.

#### Scenario: Core services are composed
- **WHEN** `NodeCoreBuilder` is constructed
- **THEN** the caller SHALL provide the chain specification, transaction-pool configuration, storage, native-contract provider, commit hooks, and persisted height
- **AND** the built core SHALL create one canonical snapshot, transaction pool, header cache, ledger context, and blockchain service from those inputs

#### Scenario: Final node assembly is attempted
- **WHEN** `NodeBuilder` is constructed
- **THEN** the caller SHALL provide the chain specification, storage, blockchain handle, network handle, transaction pool, header cache, and native-contract provider
- **AND** only genuinely optional capabilities such as wallets, a cold-ledger fallback, or an explicitly supplied staged-sync pipeline MAY be configured after construction

### Requirement: Exclusive crate ownership without compatibility facades
Each shared architecture concept SHALL have one canonical owning crate. A
migration SHALL remove superseded aggregate traits, duplicate provider traits,
type aliases, wrappers, and re-export facades unless a named external contract
requires them.

#### Scenario: Runtime consumers need chain configuration
- **WHEN** a runtime or service component needs the selected chain specification
- **THEN** it SHALL use `NeoChainSpec` and `ChainSpecProvider` from `neo-config`
- **AND** `neo-runtime` SHALL NOT define a duplicate `ConfigProvider`, `ChainSpecProvider`, `NodeTypes`, or `NeoNodeTypes` family

#### Scenario: Execution code needs the canonical VM contract type
- **WHEN** execution code constructs or inspects a NeoVM contract
- **THEN** it SHALL use `neo_vm::Contract` directly
- **AND** `neo-execution` SHALL NOT provide a wrapper, alias, or re-export facade for that type

#### Scenario: A manifest or engine uses contract call permissions
- **WHEN** NEF method tokens, execution, native contracts, or RPC scripts use `CallFlags`
- **THEN** they SHALL import the canonical type from `neo-primitives`
- **AND** `neo-manifest` SHALL NOT define or re-export a second `CallFlags` path

#### Scenario: Codec code needs a storage fixture
- **WHEN** serialization tests or callers need a memory store or snapshot
- **THEN** they SHALL import the type from `neo-storage` directly
- **AND** `neo-serialization` SHALL NOT depend on or re-export storage providers

#### Scenario: The node selects a built-in storage backend
- **WHEN** configuration selects memory or MDBX storage
- **THEN** `neo-storage` SHALL dispatch through its closed built-in backend set
- **AND** it SHALL NOT expose an unused open `StoreProvider` extension trait that conflicts with the node storage-capability trait

#### Scenario: RPC constructs a JSON-RPC exception
- **WHEN** an RPC handler constructs a code, message, and optional data error response
- **THEN** the exception type SHALL be owned by `neo-rpc`
- **AND** `neo-primitives` SHALL NOT define or re-export RPC transport errors

#### Scenario: StateRoot code needs Neo MPT mechanics
- **WHEN** a service constructs nodes, mutates a trie, or verifies an MPT proof
- **THEN** it SHALL use the backend-independent types owned by `neo-trie`
- **AND** `neo-crypto` SHALL own only the underlying Hash256 operation and SHALL NOT expose an MPT module, alias, or re-export facade
- **AND** durable MPT storage and StateService policy SHALL remain owned by `neo-state-service`

#### Scenario: An RPC handler returns an exception
- **WHEN** an RPC server handler needs a code, message, and optional data error
- **THEN** it SHALL use `neo_rpc::server::RpcException`
- **AND** `neo-primitives` SHALL NOT define or re-export RPC transport or handler exception types

#### Scenario: Network code uses a shared protocol value
- **WHEN** P2P commands or services use inventory, verification, witness, transaction-removal, or other foundation values
- **THEN** they SHALL import the canonical value from `neo-primitives`
- **AND** `neo-network` SHALL NOT wrap or re-export those values through compatibility modules
- **AND** network failures SHALL use the canonical `NetworkError` vocabulary rather than a duplicate `P2PError`

#### Scenario: The Neo P2P codec dispatches a message command
- **WHEN** a session encodes, decodes, parses, or serializes a Neo P2P command byte
- **THEN** it SHALL use the `MessageCommand` type owned by `neo-network`
- **AND** `neo-primitives` SHALL NOT define a P2P command macro, parse error, alias, or re-export facade
- **AND** unknown command bytes SHALL remain representable for forward-compatible wire handling

### Requirement: Transaction-pool policy belongs to neo-mempool
Operator-controlled transaction-pool capacity SHALL be represented by
`neo_mempool::TxPoolConfig` and SHALL remain separate from consensus chain
identity and `ProtocolSettings`.

#### Scenario: An operator configures pool capacity
- **WHEN** the application supplies a positive maximum transaction count
- **THEN** it SHALL construct a `TxPoolConfig` with that bound
- **AND** the memory pool SHALL use that configuration without changing the selected chain specification

#### Scenario: An operator configures zero pool capacity
- **WHEN** the application attempts to construct `TxPoolConfig` with a maximum transaction count of zero
- **THEN** construction SHALL fail with a typed configuration error

### Requirement: Transaction admission has one typed boundary
`neo-mempool` SHALL own one production transaction-admission operation with a
typed origin and typed validation/admission outcome. Stateless validation MAY
run before acquiring the pool write lock, but payer, Oracle, Conflicts,
state-dependent validation, and insertion SHALL remain one atomic critical
section against the same pool state.

#### Scenario: A transaction enters from a node subsystem
- **WHEN** P2P, RPC, Oracle, consensus, or another node subsystem submits a transaction
- **THEN** it SHALL identify the submission with the canonical transaction-origin type
- **AND** it SHALL use the single production admission API instead of reproducing chain-history, conflict, or pool mutation rules

#### Scenario: Stateless validation is expensive
- **WHEN** signature, wire, or other state-independent checks are performed
- **THEN** they SHALL execute before the global pool write lock is acquired
- **AND** the accepted result SHALL still be rejoined with one atomic state-dependent validation and insertion decision

#### Scenario: A required provider read fails
- **WHEN** admission cannot read canonical chain history, Policy, Oracle, payer, or conflict state
- **THEN** admission SHALL fail closed with a typed rejection or infrastructure error
- **AND** no caller SHALL reinterpret the failed read as an absent transaction or conflict

#### Scenario: Architecture guards inspect admission code
- **WHEN** workspace architecture tests scan the production transaction path
- **THEN** unused `TransactionRouter` and `PreverifyCompleted` scaffolding SHALL be absent
- **AND** duplicate public `try_add` and `try_add_cached` mutation paths SHALL NOT be restored

### Requirement: Protocol payloads are storage- and composition-independent
`neo-payloads` SHALL own canonical Neo wire payload types and their mechanical
serialization without depending on `neo-config`, `neo-storage`, or node-service
lifecycle contracts. Stateful policy reads, witness-resolution helpers, and
canonical-block observer capabilities SHALL live in their owning domain or node
service crates.

#### Scenario: A transaction or header is rendered for an address-aware boundary
- **WHEN** RPC renders a transaction or header whose address text depends on the selected chain
- **THEN** RPC SHALL pass only the required address-version value to `neo-payloads`
- **AND** `neo-payloads` SHALL render the payload without importing `neo-config`

#### Scenario: The protocol crate dependency boundary is checked
- **WHEN** workspace architecture dependencies are validated
- **THEN** the `neo-payloads` manifest and Rust sources SHALL contain no dependency on `neo-config` or `neo-storage`

#### Scenario: A transaction attribute fee is calculated
- **WHEN** a service calculates the network fee contributed by a transaction attribute
- **THEN** the service SHALL resolve the active Policy attribute fee through its native-contract provider
- **AND** `neo-payloads` SHALL receive only that resolved fee and SHALL NOT construct or query Policy storage keys

#### Scenario: A service observes canonical block lifecycle
- **WHEN** ApplicationLogs, TokensTracker, StateService, Oracle, or another node service observes committing, committed, or finalized block state
- **THEN** it SHALL implement the lifecycle capability owned by `neo-runtime`
- **AND** `neo-payloads` SHALL contain only the execution records carried by that callback

### Requirement: RPC client and server roles are independently composable
`neo-rpc` SHALL keep outbound client transport and inbound server transport as
independent Cargo features. Shared request and response records and mechanical
Neo JSON-RPC codecs SHALL live in feature-neutral modules and SHALL NOT be owned
by the client implementation.

#### Scenario: A client-only consumer builds neo-rpc
- **WHEN** `neo-rpc` is compiled with only the `client` feature
- **THEN** it SHALL compile and test without enabling server transport, node services, or server handlers

#### Scenario: A server-only consumer builds neo-rpc
- **WHEN** `neo-rpc` is compiled with only the `server` feature
- **THEN** it SHALL compile and test without enabling the client transport implementation
- **AND** server sources SHALL NOT import types through `crate::client`

#### Scenario: A type is shared by client and server
- **WHEN** both roles need the same JSON-RPC representation or address codec
- **THEN** the representation SHALL be owned by `neo_rpc::types` or `neo_rpc::protocol`
- **AND** no compatibility copy or re-export from the old client model path SHALL remain

### Requirement: Neo MPT mechanics have one exclusive owner
`neo-trie` SHALL exclusively own Neo-compatible MPT nodes, deterministic
serialization, proof verification, and the backend-independent mutation cache.
`neo-crypto` SHALL remain the owner of cryptographic hashing and SHALL NOT
re-export or duplicate the trie implementation.

#### Scenario: StateService mutates or proves Neo state
- **WHEN** StateService constructs a trie, reads a node, applies a mutation, or verifies a proof
- **THEN** it SHALL use the canonical `neo-trie` API
- **AND** durable snapshot and commit policy SHALL remain in `neo-state-service` and `neo-storage`

#### Scenario: A trie node hash is calculated
- **WHEN** `neo-trie` calculates the hash of a Neo MPT node
- **THEN** it SHALL delegate Hash256 to `neo-crypto`
- **AND** `neo-crypto` SHALL NOT depend on `neo-trie` or expose an `mpt_trie` compatibility facade

#### Scenario: The ownership boundary is validated
- **WHEN** architecture tests inspect the workspace
- **THEN** the old `neo-crypto/src/mpt_trie` implementation SHALL be absent
- **AND** canonical MPT consumers SHALL import `neo-trie` directly
