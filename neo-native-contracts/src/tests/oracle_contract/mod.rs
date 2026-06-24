//! Oracle-contract test suite, split for file-size hygiene.
//!
//! - [`native_tests`]: native-contract surface, storage codecs, request/id-list
//!   record layout, manifest-standards activation.
//! - [`request_finish_tests`]: the `request`/`finish` lifecycle (record writes,
//!   GAS minting, callback queuing, post-persist cleanup, validation faults).

mod native_tests;
mod request_finish_tests;
