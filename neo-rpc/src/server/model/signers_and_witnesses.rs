use neo_payloads::{Signer, Witness};

/// Mirrors `Neo.Plugins.RpcServer.Model.SignersAndWitnesses`, bundling a set of
/// signers together with their optional witness scripts supplied via RPC.
#[derive(Clone, Debug)]
pub struct SignersAndWitnesses {
    /// Transaction signers supplied by the caller.
    pub signers: Vec<Signer>,
    /// Witness scripts supplied by the caller.
    pub witnesses: Vec<Witness>,
}

impl SignersAndWitnesses {
    /// Create a signer/witness parameter bundle.
    #[must_use]
    pub const fn new(signers: Vec<Signer>, witnesses: Vec<Witness>) -> Self {
        Self { signers, witnesses }
    }

    /// Return the signers in caller-supplied order.
    #[must_use]
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    /// Return the witnesses in caller-supplied order.
    #[must_use]
    pub fn witnesses(&self) -> &[Witness] {
        &self.witnesses
    }
}
