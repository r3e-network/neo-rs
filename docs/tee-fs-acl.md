# TEE host filesystem ACL hardening

## Overview

Sensitive TEE host-side artifacts (monotonic counter, machine id, wallet metadata,
sealed-data directory) must not be world-readable. Unix already uses `0600`/`0700`.
Windows applies an owner-only DACL via `icacls` after create/write.

## API Reference

| Helper | Behavior |
|--------|----------|
| `fs_acl::restrict_owner_only(path)` | Unix: `chmod 0600` (file) / `0700` (dir). Windows: disable inheritance, grant only the current user Full control (dirs: `(OI)(CI)F`). |
| `fs_acl::write_owner_only(path, bytes)` | Write/truncate then `restrict_owner_only`. |

Failures are returned to callers; counter persist paths fail closed. Machine-id
persist logs a warning (same as prior Unix write-failure behavior).

## Design

- Prefer std/`icacls` over adding a `windows` ACL crate dependency.
- Domain accounts use `USERDOMAIN\USERNAME` when both env vars exist.
- Mainnet C#↔Rust differential replay remains out of scope for this hardening.

## Usage Examples

```rust
fs_acl::write_owner_only(&counter_path, &value.to_le_bytes())?;
fs_acl::restrict_owner_only(&sealed_data_dir)?;
```

## Test Coverage

- Existing `test_corrupt_monotonic_counter_fails_closed` still covers counter load.
- Unit test `restrict_owner_only_roundtrip` writes a temp file and asserts the
  helper returns `Ok` on the current platform.
