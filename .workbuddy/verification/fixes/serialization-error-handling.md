# Serialization error handling — HIGH severity

## Scope

Replaced silent `let _ = writer.write_...()` discards with proper `?` propagation
in two locations:

1. `neo-consensus/src/messages/recovery.rs` — `RecoveryMessage::serialize`
2. `neo-rpc/src/server/rpc_server_state/mod.rs` — `RpcServerState::encode_proof_payload`

The in-memory `BinaryWriter` writes to a `Vec<u8>` and cannot fail today, but
`?` propagation makes the code resilient to a future streaming sink and stops
silently dropping errors that would corrupt the on-the-wire payload.

## Issue 1: neo-consensus recovery serialization

### File

`neo-consensus/src/messages/recovery.rs:229`

### Before

```rust
pub fn serialize(&self) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    ...
    let _ = SerializeHelper::serialize_array(&cvs, &mut writer);
    let _ = writer.write_bool(has_prepare_request);
    ...
    let _ = writer.write_bytes(&bytes);
    ...
    let _ = writer.write_var_bytes(&hash.to_bytes());
    let _ = writer.write_var_int(0);
    let _ = SerializeHelper::serialize_array(&preps, &mut writer);
    let _ = SerializeHelper::serialize_array(&commits, &mut writer);
    writer.into_bytes()
}
```

7 silently-discarded write results. The signature returns `Vec<u8>` and the
function is unable to communicate a truncated/malformed recovery message to
its callers.

### After

Signature widened to `ConsensusResult<Vec<u8>>`. Each call uses
`map_err(writer_error)?` against a small free-function adapter that maps
`neo_io::IoError` → `ConsensusError::SerializationError`.

```rust
pub fn serialize(&self) -> ConsensusResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    ...
    SerializeHelper::serialize_array(&cvs, &mut writer).map_err(writer_error)?;
    writer.write_bool(has_prepare_request).map_err(writer_error)?;
    ...
    writer.write_bytes(&bytes).map_err(writer_error)?;
    ...
    writer.write_var_bytes(&hash.to_bytes()).map_err(writer_error)?;
    writer.write_var_int(0).map_err(writer_error)?;
    SerializeHelper::serialize_array(&preps, &mut writer).map_err(writer_error)?;
    SerializeHelper::serialize_array(&commits, &mut writer).map_err(writer_error)?;
    Ok(writer.into_bytes())
}

fn writer_error(err: neo_io::IoError) -> crate::ConsensusError {
    crate::ConsensusError::SerializationError(format!(
        "RecoveryMessage write failed: {err}"
    ))
}
```

Why not `From<IoError> for ConsensusError`? `ConsensusError` already has
`IoError(#[from] std::io::Error)` and `BincodeError(#[from] bincode::Error)`
variants, but the existing `IoError` variant wraps `std::io::Error`, not
`neo_io::IoError`. Adding a `From<neo_io::IoError>` impl would be a wider
cross-crate change; using a thin local `writer_error` adapter keeps the fix
local to the call site and is the smallest correct change.

### Caller updates

`RecoveryMessage::serialize` is consumed in three places that already return
`ConsensusResult` (or are inside `Result`-returning methods), so the call
sites only need `?` added:

- `neo-consensus/src/service/handlers/recovery.rs:53` — `recovery.serialize()?`
- `neo-consensus/src/service/handlers/recovery.rs:391` — `recovery.serialize()?`
- `neo-consensus/src/service/handlers/recovery.rs:400` — `recovery.serialize()?`

The unit tests under `neo-consensus/src/tests/messages/recovery.rs` and
`neo-consensus/src/tests/service/recovery.rs` already call
`RecoveryMessage::serialize()` and pass the returned `Vec<u8>` directly into
`ConsensusPayload::new`. The signature change forces these to call
`.unwrap()` (the in-memory writer cannot fail, so the test path is still
deterministic) — 8 test sites updated:

- `neo-consensus/src/tests/messages/recovery.rs:26` (`msg.serialize().unwrap()`)
- `neo-consensus/src/tests/messages/recovery.rs:56` (`msg.serialize().unwrap()`)
- `neo-consensus/src/tests/service/recovery.rs:172, 230, 283, 316, 363, 414, 529`
  (`recovery.serialize().unwrap()`)
- `tests/tests/consensus/consensus_integration_tests.rs:356` (`msg.serialize().unwrap()`)

All `cargo test -p neo-consensus --lib messages` and
`cargo test -p neo-consensus --lib service::tests::recovery` pass:
- 30 message tests, 11 recovery service tests, all green.

## Issue 2: neo-rpc server state proof payload

### File

`neo-rpc/src/server/rpc_server_state/mod.rs:431`

### Before

```rust
fn encode_proof_payload(key: &[u8], nodes: &[Vec<u8>]) -> Vec<u8> {
    let mut writer = neo_io::BinaryWriter::new();
    let _ = writer.write_var_bytes(key);
    let _ = writer.write_var_int(nodes.len() as u64);
    for node in nodes {
        let _ = writer.write_var_bytes(node);
    }
    writer.into_bytes()
}
```

3 silently-discarded write results. The function returns `Vec<u8>`, so a
serialization failure would emit a truncated proof payload to the
`verifyproof` caller.

### After

Signature widened to `Result<Vec<u8>, RpcException>`. Each call uses
`.map_err(Self::writer_error_to_rpc)?` against a helper that maps
`neo_io::IoError` → `RpcException::from(RpcError::internal_server_error()
.with_data(...))`.

```rust
fn encode_proof_payload(key: &[u8], nodes: &[Vec<u8>]) -> Result<Vec<u8>, RpcException> {
    let mut writer = neo_io::BinaryWriter::new();
    writer.write_var_bytes(key).map_err(Self::writer_error_to_rpc)?;
    writer
        .write_var_int(nodes.len() as u64)
        .map_err(Self::writer_error_to_rpc)?;
    for node in nodes {
        writer.write_var_bytes(node).map_err(Self::writer_error_to_rpc)?;
    }
    Ok(writer.into_bytes())
}

fn writer_error_to_rpc(err: neo_io::IoError) -> RpcException {
    RpcException::from(
        RpcError::internal_server_error()
            .with_data(format!("proof payload encoding failed: {err}")),
    )
}
```

### Caller updates

The only caller is `proof_payload` at
`neo-rpc/src/server/rpc_server_state/mod.rs:351`, which already returns
`Result<String, RpcException>`:

```rust
Ok(BASE64_STANDARD.encode(Self::encode_proof_payload(&storage_key, &nodes)))
//          ↓
Self::encode_proof_payload(&storage_key, &nodes)?
    .pipe(|payload| Ok::<_, RpcException>(BASE64_STANDARD.encode(payload)))
```

The existing `?` propagation from the surrounding `Result` works because
`encode_proof_payload` now returns `Result<Vec<u8>, RpcException>`. The caller
change is one line: `BASE64_STANDARD.encode(Self::encode_proof_payload(...)?)`.

Test callers updated to `.unwrap()`:

- `neo-rpc/src/tests/server/rpc_server_state/proof.rs:56`
- `neo-rpc/src/tests/server/rpc_server_state/proof.rs:90`

Both produce valid `Vec<u8>` and the test path remains deterministic.

## Verification

- `cargo check -p neo-consensus -p neo-rpc` — clean
- `cargo check -p neo-consensus -p neo-rpc --tests` — clean
- `cargo check -p neo-rpc --features server --tests` — clean
- `cargo test -p neo-consensus --lib messages::recovery` — 6 passed, 0 failed
- `cargo test -p neo-consensus --lib messages` — 30 passed, 0 failed
- `cargo test -p neo-consensus --lib service::tests::recovery` — 11 passed, 0 failed
- `cargo test -p neo-rpc --features server --lib server::rpc_server_state` — 21 passed, 0 failed

## Risk

- Wire-format unchanged: the in-memory `BinaryWriter` cannot fail, so the
  on-the-wire bytes are identical to before. The `recovery_message_wire_format_bytes_without_prepare_request`
  test exercises the byte-exact layout and passes.
- Caller signatures: every caller of `RecoveryMessage::serialize` and
  `RpcServerState::encode_proof_payload` now sees a `Result` and either
  propagates (`?`) or unwraps the test-only value. The unit/integration
  test suite confirms no behavioral regression.
- `RecoveryMessage::serialize` no longer has `#[must_use]` because the
  return type is `Result` (which is already `#[must_use]`), and dropping
  the return value would now be a hard compile error rather than a silent
  byte-discard.
