use neo_core::network::p2p::payloads::{Signer, Witness};

/// Mirrors `Neo.Plugins.RpcServer.Model.SignersAndWitnesses`, bundling a set of
/// signers together with their optional witness scripts supplied via RPC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignersAndWitnesses {
    pub signers: Vec<Signer>,
    pub witnesses: Vec<Witness>,
}

impl SignersAndWitnesses {
    pub fn new(signers: Vec<Signer>, witnesses: Vec<Witness>) -> Self {
        Self { signers, witnesses }
    }

    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }
}
