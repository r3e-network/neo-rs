// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;


/// Terms in DBFT v2.0
/// ConsensusNode:	Nodes that can propose a new block and vote for the proposed block.
/// NormalNode: Nodes that can transfer and create transactions, are also ledges, but can neither propose new blocks nor vote.
/// Speaker:	Validator in charge of creating and broadcasting a proposal block to the network.
/// Delegate:	Validator responsible for voting on the block proposal.
/// Candidate:	Account nominated for validator election.
/// Validator:	Account elected from candidates to take part in consensus.
/// View:	Referred to the dataset used during a round of consensus.
///   View number v starts from 0 in each round and increases progressively upon consensus failure
///   until the approval of the block proposal, and then is reset to 0.
pub mod dbft_v2;

use neo_core::{block, payload::Extensible, tx::Tx, types::Sign};


pub struct Block {
    pub network: u32,
    pub block: block::Block,
    pub sign: Sign,
}


pub trait Consensus {
    type OnPayloadError;
    type OnTxError;

    fn name(&self) -> &str;

    fn start(&mut self);

    fn stop(&mut self);

    fn on_payload(&mut self, payload: &Extensible) -> Result<(), Self::OnPayloadError>;

    fn on_tx(&mut self, tx: &Tx) -> Result<(), Self::OnTxError>;
}

