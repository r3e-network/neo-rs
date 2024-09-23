use crate::rpcclient::unwrap;
use crate::util::Uint160;
use crate::rpcclient::{Invoker, Actor};
use crate::base::{BaseReader, BaseWriter};

// NonDivisibleReader is a reader interface for non-divisible NEP-11 contract.
pub struct NonDivisibleReader {
    base_reader: BaseReader,
}

// NonDivisible is a state-changing interface for non-divisible NEP-11 contract.
pub struct NonDivisible {
    non_divisible_reader: NonDivisibleReader,
    base_writer: BaseWriter,
}

// NewNonDivisibleReader creates an instance of NonDivisibleReader for a contract
// with the given hash using the given invoker.
impl NonDivisibleReader {
    pub fn new(invoker: Arc<dyn Invoker>, hash: Uint160) -> Self {
        NonDivisibleReader {
            base_reader: BaseReader::new(invoker, hash),
        }
    }

    // OwnerOf returns the owner of the given NFT.
    pub fn owner_of(&self, token: &[u8]) -> Result<Uint160, Box<dyn std::error::Error>> {
        unwrap::uint160(self.base_reader.invoker.call(self.base_reader.hash, "ownerOf", token))
    }
}

// NewNonDivisible creates an instance of NonDivisible for a contract
// with the given hash using the given actor.
impl NonDivisible {
    pub fn new(actor: Arc<dyn Actor>, hash: Uint160) -> Self {
        NonDivisible {
            non_divisible_reader: NonDivisibleReader::new(actor.clone(), hash),
            base_writer: BaseWriter::new(hash, actor),
        }
    }
}
