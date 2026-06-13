# Code Conventions

## Error Handling

The workspace uses a layered error strategy:

### Cross-crate errors
Use `CoreError`/`CoreResult` from `neo_error` for errors that cross crate boundaries.

```rust
use neo_error::{CoreError, CoreResult};
```

**Do NOT alias** `CoreError` to `Error` or `CoreResult` to `Result`. Use the full names.

### Crate-internal domain errors
Each crate may define its own `thiserror` types for errors that don't leave the crate boundary:

```rust
#[derive(Debug, thiserror::Error)]
pub enum MyCrateError {
    #[error("specific error: {0}")]
    Specific(String),
}
```

### Application boundary
Only `neo-node` (the binary) may use `anyhow`. Library crates must use typed errors.

## Lint Directives

Every crate must have these directives at the top of `lib.rs` (or `main.rs`):

```rust
#![deny(unsafe_code)]
#![warn(missing_docs)]
```

If a crate genuinely needs unsafe code, add a targeted `#[allow(unsafe_code)]` with a `// SAFETY:` comment on the specific item.

## Dependencies

All internal dependencies must use `workspace = true`:

```toml
# Correct
neo-primitives = { workspace = true }

# Wrong
neo-primitives = { path = "../neo-primitives" }
```

## Macros

The `impl_default_via_new!` macro is defined once in `neo-io/src/macros.rs`. All crates use:

```rust
neo_io::impl_default_via_new!(MyType);
```
