/// Module gas provides a convenience wrapper for GAS contract to use it via RPC.
///
/// GAS itself only has standard NEP-17 methods, so this module only contains its
/// hash and allows to create NEP-17 structures in an easier way. Refer to [nep17]
/// module for more details on NEP-17 interface.

pub mod gas {
    use crate::core::native::nativehashes;
    use crate::rpcclient::nep17;

    /// Hash stores the hash of the native GAS contract.
    pub const HASH: [u8; 20] = nativehashes::GAS_TOKEN;

    /// NewReader creates a NEP-17 reader for the GAS contract.
    pub fn new_reader(invoker: nep17::Invoker) -> nep17::TokenReader {
        nep17::new_reader(invoker, HASH)
    }

    /// New creates a NEP-17 contract instance for the native GAS contract.
    pub fn new(actor: nep17::Actor) -> nep17::Token {
        nep17::new(actor, HASH)
    }
}
