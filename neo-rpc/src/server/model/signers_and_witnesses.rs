use neo_core::network::p2p::payloads::{Signer, Witness};

/// Mirrors `Neo.Plugins.RpcServer.Model.SignersAndWitnesses`, bundling a set of
/// signers together with their optional witness scripts supplied via RPC.
#[derive(Clone, Debug)]
pub struct SignersAndWitnesses {
    pub signers: Vec<Signer>,
    pub witnesses: Vec<Witness>,
}

impl SignersAndWitnesses {
    #[must_use] 
    pub const fn new(signers: Vec<Signer>, witnesses: Vec<Witness>) -> Self {
        Self { signers, witnesses }
    }

    #[must_use] 
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    #[must_use] 
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }
}
