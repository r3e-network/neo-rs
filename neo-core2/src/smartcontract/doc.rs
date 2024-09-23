//! This module contains functions to deal with widely used scripts and NEP-14 Parameters.
//!
//! Neo is all about various executed code, verifications and executions of
//! transactions need NeoVM code and this module simplifies creating it
//! for common tasks like multisignature verification scripts or transaction
//! entry scripts that call previously deployed contracts. Another problem related
//! to scripts and invocations is that RPC invocations use JSONized NEP-14
//! parameters, so this module provides types and methods to deal with that too.

// The `package smartcontract` line is not needed in Rust as the module is defined by the file itself.
