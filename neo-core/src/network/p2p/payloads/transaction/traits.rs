//
// traits.rs - neo-core local trait implementations for Transaction
//
// The canonical `Transaction` is owned by `neo-protocol`, which already
// provides the structural trait impls: `Serializable`, `SerializablePayload`,
// `Inventory`, `neo_primitives::Verifiable`, `Default`, `std::hash::Hash`, and
// its own `neo_protocol::witness::WitnessData`. Re-declaring any of those here
// would be a duplicate impl on a foreign type.
//
// neo-core keeps only its *local* `crate::witness::WitnessData` trait, which is
// a distinct trait from neo-protocol's. Because both traits share an identical
// signature and the underlying `Witness` type is the same re-export, we simply
// delegate to neo-protocol's implementation.
//

use super::Transaction;
use crate::witness::{Witness, WitnessData};

impl WitnessData for Transaction {
    fn witnesses(&self) -> Vec<&Witness> {
        <Transaction as neo_protocol::witness::WitnessData>::witnesses(self)
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        <Transaction as neo_protocol::witness::WitnessData>::witnesses_mut(self)
    }
}
