# Code Conventions

Shared standards for every crate in the workspace. Consistency is enforced where
possible (centralized lints, `cargo fmt`/`clippy` in CI) and documented here
where it cannot be.

## Lints

The lint policy lives in **one** place — the root `Cargo.toml`:

```toml
[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "warn"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
# … pragmatic allows (see the manifest)
```

Every member opts in with:

```toml
[lints]
workspace = true
```

Do **not** re-add `#![deny(unsafe_code)]` / `#![warn(missing_docs)]` headers to
`lib.rs`/`main.rs` — they are inherited from the workspace.

- **Unsafe:** the two crates that need it (`neo-vm`, `neo-execution`) inherit the
  `deny` and opt out **per-site** with `#[allow(unsafe_code)]` plus a `// SAFETY:`
  comment stating the invariant — never a blanket crate-level opt-out.
- **Silencing a lint:** prefer `#[expect(lint, reason = "…")]` over
  `#[allow(lint)]` — `expect` warns when the suppression becomes stale. Never add
  a crate-level `#![allow(dead_code)]` / `#![allow(unused_imports)]`; delete the
  dead code or scope the allow to the specific item with a reason.

## Error Handling

Layered strategy:

### Cross-crate errors
Use `CoreError`/`CoreResult` from `neo_error` for errors that cross crate
boundaries. Spell the names in full:

```rust
use neo_error::{CoreError, CoreResult};
```

**Do NOT alias** `CoreError` to `Error` or `CoreResult` to `Result`, and do not
define a crate-local `pub type Result<…>` alias.

### Crate-internal domain errors
Each crate defines its own `thiserror` enum for errors that don't leave the
crate. Never hand-roll `Display`/`std::error::Error`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum MyCrateError {
    #[error("specific error: {0}")]
    Specific(String),
    #[error("decode failed")]
    Decode(#[from] SomeLowerError),   // use #[from] to wrap lower-level errors
    #[error(transparent)]             // or transparent to delegate Display
    Other(#[from] AnotherError),
}
```

- A module with exactly one error case uses a `struct`, not a one-variant enum.
- Errors crossing an `.await`/spawn boundary must be `Send + Sync + 'static`.
- `Result<_, String>` is forbidden in library code — map to a `CoreError` or a
  crate `thiserror` variant. Preserve C#-parity fault-message text where present.

### Application boundary
Only `neo-node` (the binary) may use `anyhow`. Library crates use typed errors.

### Panics
No `unwrap`/`expect`/`panic!` in library code for recoverable conditions —
return a typed error and bubble with `?`. `unreachable!` is acceptable only with
a proven invariant. `todo!`/`unimplemented!` and `// TODO` comments are
forbidden in committed code (file an issue instead).

## Dependencies & Manifests

- All internal **and** workspace-pinned external deps use `{ workspace = true }`,
  never `{ path = … }` or a duplicated version literal:
  ```toml
  neo-primitives = { workspace = true }   # correct
  ```
- Use the inline-table form `foo = { workspace = true }`, not the dotted
  `foo.workspace = true`, in dependency tables.
- `[package]` inherits every shared key via `.workspace = true` (version,
  edition, rust-version, authors, homepage, repository, documentation, license,
  keywords, categories, readme). The only per-crate `[package]` literal is
  `description`.

## Macros

Shared macros live in `neo-io/src/macros.rs`; use them instead of hand-rolling
boilerplate when the type qualifies (non-generic, plain fields):

```rust
neo_io::impl_default_via_new!(MyType);   // Default that is just Self::new()
```

They do **not** apply to generic/bounded types, derived-value ordering,
pointer-identity hashing, or consensus-critical custom codecs — keep those
hand-written.

## Module & File Organization

- One independent public type per self-named file. `lib.rs`/`mod.rs` are
  re-export hubs with explicit `pub use sub::Type;` — never `pub use sub::*;`
  (a flat constants module is the one accepted exception). Tightly-coupled
  clusters (a channel-actor `Handle`+`Service`+`Command`, C#-parity nested
  sub-payloads) may co-locate.

## Documentation

- Crate doc opens with `//! # neo-<crate-name>` then a one-line summary.
- Public items use `///` in third-person present voice ("Creates", "Returns",
  "Decodes"). Add `# Errors`/`# Panics`/`# Safety` sections where applicable.
- `// SAFETY:` on every unsafe site. C#-parity references (`C# Neo.X.Y`) are
  intentional, keep them.

## Naming & API Idioms

- Constructors: `new()` infallible, `try_new()` fallible, `with_*()` variant,
  `from_*()` conversion. `create_*` only for C#-parity factory mirrors.
- Getters are bare `field()` (no `get_` prefix); setters are `set_field()`.
- Conversions: `as_*` cheap borrow / `to_*` owned clone / `into_*` consuming.
- Extension traits end in `*Ext`; predicates use `is_`/`has_`/`can_`.

## Tests

- Unit tests are inline `#[cfg(test)] mod tests { … }`; a crate `tests/` dir is
  integration-only (one binary per file, no aggregator `mod.rs`).
- Test fns use descriptive snake_case behaviour names, no `test_` prefix.
- `#[tokio::test]` by default; `flavor = "multi_thread"` only for real
  server/IO runtimes. C#-parity vectors are named `*_matches_csharp` in
  `*_pinning.rs` files.
