# Style & conventions
- Rust idioms, derives for serde/Clone/Copy/Eq/Hash; error handling via thiserror and PrimitiveResult alias.
- Little-endian byte order for UInt160/UInt256 (hex strings reversed before parsing, to_array returns LE).
- Logging via `tracing::error` inside len-validation helpers `from_span`.
- Module-level constants for protocol values; ADDRESS_VERSION currently 0x35 (Neo N3).
- Tests use std `assert` (no snapshot/insta); no custom lint config observed.