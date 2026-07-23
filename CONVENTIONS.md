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

- **Unsafe:** workspace default remains `deny`. A crate may opt out **per-site**
  with `#[allow(unsafe_code)]` only for a measured hot path or unavoidable FFI
  boundary. Every site needs a `// SAFETY:` invariant, a safe public wrapper, and
  benchmark/parity evidence in the PR or commit notes. Never use a blanket
  crate-level opt-out for convenience.
- **Silencing a lint:** prefer `#[expect(lint, reason = "…")]` over
  `#[allow(lint)]` — `expect` warns when the suppression becomes stale. Never add
  a crate-level `#![allow(dead_code)]` / `#![allow(unused_imports)]`; delete the
  dead code or scope the allow to the specific item with a reason.

## Error Handling

The error strategy is formalized in **ADR-011** (see `design.md`). The layered
strategy works as follows:

### Cross-crate errors
Use `CoreError`/`CoreResult` from `neo_error` for errors that cross crate
boundaries. Spell the names in full:

```rust
use neo_error::{CoreError, CoreResult};
```

**Do NOT alias** `CoreError` to `Error` or `CoreResult` to `Result`, and do not
define a crate-local `pub type Result<…>` alias.

### Crate-internal domain errors (ADR-011 Rule 1)
A crate **MUST** define its own `thiserror` error type when it has
domain-specific failure modes that callers need to match on. This includes
crypto, storage, VM, consensus, network, HSM, and TEE operations. Every domain
error type **MUST** implement `From<DomainError> for CoreError` for seamless
`?` propagation (ADR-003).

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

### CoreError-direct crates (ADR-011 Rule 2)
A crate **MAY** use `CoreError`/`CoreResult` directly when its failures are
generic validation or codec errors with no domain-specific variants callers need
to match on. This includes `neo-payloads`, `neo-native-contracts`, `neo-mempool`,
`neo-blockchain`, `neo-state-service`, `neo-manifest`, `neo-execution`, and
`neo-serialization`. Never hand-roll `Display`/`std::error::Error`.

- A module with exactly one error case uses a `struct`, not a one-variant enum.
- Errors crossing an `.await`/spawn boundary must be `Send + Sync + 'static`.
- `Result<_, String>` is forbidden in library code — map to a `CoreError` or a
  crate `thiserror` variant. Preserve C#-parity fault-message text where present.
- The 17/9 split (17 domain-error crates, 9 CoreError-direct crates) is policy,
  not accident. See ADR-011 for the decision tree.

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
- Top-level orchestration should read as domain flow and push mechanics down to
  lower modules. See `docs/coding-design-architecture-guidance.md` for the
  chained workflow, abstraction, generics, and `dyn Trait` rules.
- Keep runtime data out of git. Local ledgers, MDBX environments, checkpoints,
  downloaded fast-sync archives, replay output, and logs are generated
  artifacts; commit only source, docs, scripts, and small deterministic fixtures
  with an explicit test purpose.

## Documentation

- Crate doc opens with `//! # neo-<crate-name>` then a one-line summary.
- Public items use `///` in third-person present voice ("Creates", "Returns",
  "Decodes"). Add `# Errors`/`# Panics`/`# Safety` sections where applicable.
- `// SAFETY:` on every unsafe site. C#-parity references (`C# Neo.X.Y`) are
  intentional, keep them.
- `#![doc(html_root_url = "https://docs.rs/neo-<crate>/<version>")]` — the
  version **MUST** match `workspace.package.version` in the root `Cargo.toml`
  (ADR-013). Currently `0.11.0`. Do not let this drift on release.
- Do **not** re-add `#![deny(unsafe_code)]` / `#![warn(missing_docs)]` headers
  to `lib.rs`/`main.rs` or submodule `mod.rs` — they are inherited from the
  workspace lints. (Only benchmark/test harness files may use
  `#![allow(missing_docs)]`.)

## Naming & API Idioms

- Constructors: `new()` **MAY** be fallible; reserve `try_new()` only where an
  infallible `new()` also exists on the same type. `with_*()` variant,
  `from_*()` conversion. `create_*` only for C#-parity factory mirrors.
- Getters are bare `field()` (no `get_` prefix); setters are `set_field()`.
  Exception: `neo-rpc/src/client/**` accessor names deliberately mirror
  JSON-RPC method names (`get_*`) and are exempt from the bare-accessor rule.
- Conversions: `as_*` cheap borrow / `to_*` owned clone / `into_*` consuming.
- Extension traits end in `*Ext`; predicates use `is_`/`has_`/`can_`.

## Tests

- Unit tests are inline `#[cfg(test)] mod tests { … }`; a crate `tests/` dir is
  integration-only (one binary per file, no aggregator `mod.rs`).
- Test fns use descriptive snake_case behaviour names, no `test_` prefix.
- `#[tokio::test]` by default; `flavor = "multi_thread"` only for real
  server/IO runtimes. C#-parity vectors are named `*_matches_csharp` in
  `*_pinning.rs` files.
