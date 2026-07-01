//! # neo-serialization::tests::json
//!
//! Test module grouping JSON models and codecs for external service
//! integration. coverage for neo-serialization.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-serialization; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `jarray_tests`: JSON array coverage.
//! - `jobject_tests`: JSON object coverage.
//! - `jpath_tests`: JSON path coverage.
//! - `jtoken_tests`: JSON token coverage.
//! - `serialization_tests`: serialization integration coverage.

mod jarray_tests;
mod jobject_tests;
mod jpath_tests;
mod jtoken_tests;
mod serialization_tests;
