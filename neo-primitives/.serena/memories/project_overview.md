# neo-primitives overview
- Rust crate providing fundamental Neo blockchain primitives (UInt160/UInt256 script hash & transaction hash types, hardfork enum, protocol constants, error helpers).
- Located at /home/neo/git/neo-rs/neo-primitives; part of the larger neo-rs workspace (workspace-managed package metadata in Cargo.toml).
- Modules: constants (protocol sizes/limits including ADDRESS_SIZE/HASH_SIZE/ADDRESS_VERSION), error (PrimitiveError + PrimitiveResult alias), hardfork (network fork enum + parsing), uint160/uint256 (LE-encoded 160/256-bit integer structs with serde + conversions).
- Crate re-exports core types and sizes from lib.rs.