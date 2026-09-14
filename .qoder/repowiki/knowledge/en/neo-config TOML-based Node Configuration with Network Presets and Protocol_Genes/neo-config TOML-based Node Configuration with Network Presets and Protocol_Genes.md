---
kind: configuration_system
name: neo-config TOML-based Node Configuration with Network Presets and Protocol/Genesis Hard-coding
category: configuration_system
scope:
    - '**'
source_files:
    - neo-config/src/lib.rs
    - neo-config/src/settings.rs
    - neo-config/src/network.rs
    - neo-config/src/protocol.rs
    - neo-config/src/genesis.rs
    - neo-config/src/error.rs
    - config/mainnet.toml
    - config/testnet.toml
    - config/local.toml
    - config/mainnet-full-validation.toml
    - config/mainnet-protocol-consistency.toml
    - config/mainnet-stateroot.toml
    - config/perf-validate.toml
---

## What system/approach is used

The Neo N3 node uses a dedicated `neo-config` Rust crate as the single source of truth for runtime configuration. Configuration is expressed in **TOML files** (via the `toml` crate) and deserialized into strongly-typed serde structs. The crate provides three layers:
1. **Network presets** (`NetworkType::MainNet/TestNet/Private`) that supply default magic numbers, seed nodes, ports, and address versions.
2. **Protocol settings** (`ProtocolSettings`) hard-coded to match the live Neo N3 C# reference implementation (validators, hardfork heights, block timing, gas distribution).
3. **Node settings** (`Settings`, `StorageSettings`, `RpcSettings`, `ConsensusSettings`, `LoggingSettings`, `TelemetrySettings`, `GenesisConfig`) loaded from per-environment TOML profiles under `config/`.

There is no environment-variable override layer or secret-manager integration in this crate; secrets such as `consensus.wallet_password` are marked `#[serde(skip_serializing)]` and documented as "should be loaded from a secure source, e.g. env/secret manager" — but the loader itself does not read them.

## Key files and packages

- `neo-config/src/lib.rs` — re-exports `Settings`, `NetworkConfig`, `ProtocolSettings`, `GenesisConfig`, `HardforkHeights`, `NativeActivationHeights`, and the `CONFIG_VERSION = 1` marker.
- `neo-config/src/settings.rs` — defines the top-level `Settings` struct and all sub-settings structs with `Default` implementations and `validate()` constraints (non-zero P2P/RPC ports, required consensus wallet path, genesis validation).
- `neo-config/src/network.rs` — `NetworkType` enum with magic/address-version/seed defaults and `NetworkConfig` supporting optional overrides via `magic` / `address_version` fields.
- `neo-config/src/protocol.rs` — `ProtocolSettings::mainnet()` / `testnet()` / `private()` constructors containing the authoritative validator sets, hardfork height map, and protocol constants; includes a string-matched `is_hardfork_enabled` helper with an inline maintenance note requiring every new hardfork field to get a corresponding match arm.
- `neo-config/src/genesis.rs` — `GenesisConfig` with mainnet/testnet/private builders, validator/committee lists, and `validate()` enforcing at least one validator with a 66-char hex public key.
- `neo-config/src/error.rs` — `ConfigError` enum (file-not-found, TOML parse/serialize, invalid value, missing field, unknown network, genesis error, protocol error, validation error) built on `thiserror`.
- `config/*.toml` — per-environment profiles: `mainnet.toml`, `testnet.toml`, `local.toml`, `mainnet-full-validation.toml`, `mainnet-protocol-consistency.toml`, `mainnet-stateroot.toml`, `perf-validate.toml`, plus temporary `tmp-*.toml` files.
- `neo-rpc/src/server/rpc_server_node/mod.rs` — contains a comment explicitly noting that the RPC server's separate `HardforkHeights` representation must stay in sync with `neo-config::HardforkHeights`, documenting the cross-crate coupling.

## Architecture and conventions

- **Presets-first loading**: `Settings::for_network(NetworkType)` builds a complete baseline from hard-coded mainnet/testnet/private presets, then `Settings::from_file(path)` overlays a TOML profile on top. The TOML file is validated after parsing via `settings.validate()`, which delegates to `GenesisConfig::validate()`.
- **Strict defaults**: Every setting struct implements `Default` with sensible production values (e.g. `p2p_port = 10333` for mainnet, `rpc.port = 10332`, storage path under `dirs::data_dir()/neo-rs`, RocksDB cache 256 MB, compression enabled). TOML fields use `#[serde(default)]` so partial profiles are accepted.
- **Network identity separation**: `NetworkType` carries canonical magic/address-version/seed data; `NetworkConfig` allows overriding `magic` and `address_version` while keeping `effective_magic()` / `effective_address_version()` as the single accessor used by callers.
- **Hardforks as opt-in heights**: `HardforkHeights` fields are `Option<u32>`; `is_hardfork_enabled(name, height)` returns false when the height is unset, so new hardforks are disabled by default until a height is configured.
- **Validation gate**: All load paths (`from_file`, `from_toml_str`, `FromStr`) call `validate()` before returning, ensuring port non-zero, consensus wallet presence, and genesis integrity.
- **Round-trip support**: `save()` / `to_toml()` serialize back to pretty-printed TOML, enabling config generation and diffing.
- **Configuration versioning**: `CONFIG_VERSION = 1` is exported as a migration marker for future schema changes.

## Conventions and constraints

- **TOML-only config format**: All user-facing configuration lives in `.toml` files under `config/`; there is no YAML/JSON/env-file loader in `neo-config`.
- **Environment-specific profiles**: Each deployment target gets its own file in `config/` (e.g. `mainnet.toml`, `testnet.toml`, `local.toml`, `mainnet-full-validation.toml`). Temporary/bisect runs use `tmp-*.toml` files.
- **Secrets excluded from serialization**: `ConsensusSettings::wallet_password` is `#[serde(skip_serializing)]` and documented as intended to be injected from an external secret store rather than persisted in TOML.
- **Validator set immutability in code**: Mainnet and testnet validator public keys are hard-coded in `ProtocolSettings::mainnet()` / `testnet()`; they are not read from TOML, so changing validators requires a code change and rebuild.
- **Hardfork activation heights are code-gated**: New hardforks must add both a field to `HardforkHeights` and a match arm in `is_hardfork_enabled`; the comment in `protocol.rs` explicitly calls out this requirement.
- **Port assignment convention**: Mainnet uses 10333 (P2P) / 10332 (RPC), TestNet uses 20333 / 20332, Private uses 30333 / 30332 — enforced in `Settings::for_network`.
- **Cross-crate hardfork sync contract**: `neo-rpc` maintains a parallel `HardforkHeights` type and comments require it to be updated whenever `neo-config::HardforkHeights` gains a field, indicating a manual synchronization convention between crates.