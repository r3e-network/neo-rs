mod model;
mod traits;

pub use model::Address;
pub use traits::{ToNeo3Address, ToScriptHash, ToSignData};

pub const ADDRESS_NEO3: u8 = 0x35;
