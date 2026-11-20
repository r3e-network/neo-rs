//! Application-level fast path routing for recognized transaction templates.
//!
//! These modules own the *application semantics* of the fast path: recognizing
//! the standard single-call NEP-17 transfer template, classifying a transaction
//! into an execution complexity, and describing recognized system block scripts.
//! They are classification and metrics infrastructure, not an execution
//! accelerator: direct execution lives in `direct_nep17.rs` and
//! `execution_strategy.rs`, and the canonical `ApplicationEngine` remains the
//! only producer of consensus state.
//!
//! The generic bytecode walking (opcode matching, `PUSHDATA1` and integer
//! decoding) stays in `neo_vm::fast_path::InstructionCursor`; nothing here
//! duplicates instruction decoding.
//!
//! ## Safety
//!
//! Direct dispatch is only reachable for scripts the strict parser accepted.
//! Any other script — including a transfer template with extra instructions —
//! falls back to the canonical `ApplicationEngine` path, which remains the only
//! producer of consensus state.

pub mod block_scripts;
pub mod classifier;
pub mod executor;
pub mod nep17_parser;

pub use block_scripts::BlockScriptExecutor;
pub use classifier::{BlockScriptType, TransactionClassifier, TransactionComplexity};
pub use executor::{FastPathClassifier, FastPathStats};
pub use nep17_parser::{Nep17Parser, TransferParams};
