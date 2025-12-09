# Style and conventions
- Rust 2021 idioms; uses derive traits for enums (Debug, Clone, Copy, Eq, Hash, Serialize, Deserialize) and `thiserror::Error` for error enums with display formatting.
- Enum modules include helper conversion methods (`from_byte`, `to_byte`, `as_str`) and `Display` implementations; defaults are explicit (`ChangeViewReason` defaults to Timeout).
- Doc comments used for public items; small inline tests placed in `#[cfg(test)]` modules within the same file.
- No unsafe code used; expect standard Rust formatting via rustfmt and linting with clippy (workspace defaults).
- Serialization derives rely on serde; numeric representations fixed with `#[repr(u8)]` for message enums to match Neo C# wire values.
