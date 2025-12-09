# neo-consensus
- Purpose: Crate in the neo-rs workspace intended to host dBFT 2.0 consensus logic for Neo N3. Current contents are limited to type definitions and errors; no protocol implementation yet.
- Tech stack: Rust 2021 (workspace rust-version 1.75, MSRV 1.70 per workspace metadata), serde for serialization, thiserror for error display, tracing/tokio available via workspace deps though unused here.
- Structure: `src/lib.rs` re-exports modules; `src/message_type.rs` defines `ConsensusMessageType` enum with byte conversions and Display; `src/change_view_reason.rs` defines `ChangeViewReason`; `src/error.rs` defines `ConsensusError` and `ConsensusResult`. Placeholder commented modules for service/context/messages.
- Integration: Part of mono-repo workspace at /home/neo/git/neo-rs with many peer crates (neo-core, neo-p2p, etc.). No binaries/entrypoints in this crate; it is library-only.
- Tests: Unit tests verifying enum value mappings and string conversions live at bottom of the enum modules; no integration or protocol tests yet.
- Security: No unsafe code present at this stage; protocol logic not implemented yet.
