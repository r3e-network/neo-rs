use sha2::{Sha256, Digest};
use std::convert::TryInto;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
struct NotFoundError;

impl fmt::Display for NotFoundError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "interop not found")
    }
}

impl Error for NotFoundError {}

// ToID returns an identifier of the method based on its name.
fn to_id(name: &[u8]) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(name);
    let result = hasher.finalize();
    u32::from_le_bytes(result[0..4].try_into().expect("slice with incorrect length"))
}

// FromID returns interop name from its id.
fn from_id(id: u32, names: &[&str]) -> Result<String, Box<dyn Error>> {
    for &name in names {
        if id == to_id(name.as_bytes()) {
            return Ok(name.to_string());
        }
    }
    Err(Box::new(NotFoundError))
}
