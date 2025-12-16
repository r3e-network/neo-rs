//! Block Executor Module
//!
//! Production-ready block execution with full transaction processing
//! and state change extraction for MPT state root calculation.
//!
//! ## Execution Flow
//!
//! ```text
//! Block Execution:
//! 1. OnPersist    - System trigger for native contract persistence
//! 2. Application  - Execute each transaction in block
//! 3. PostPersist  - System trigger for post-block cleanup
//! 4. Commit       - Persist state changes and calculate state root
//! ```

mod block_executor;
mod key_converter;
mod types;

pub use block_executor::BlockExecutorImpl;
