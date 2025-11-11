mod keypair;
mod private;
mod public;

pub use keypair::{KeyError, Keypair};
pub use private::PrivateKey;
pub use public::PublicKey;

pub const KEY_SIZE: usize = 32;

#[cfg(test)]
mod tests;
