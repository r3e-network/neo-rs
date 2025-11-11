extern crate alloc;

mod error;
mod ops;
mod prehash;

pub use error::Secp256k1Error;
pub use ops::{recover_public_key, sign, sign_recoverable, verify};
