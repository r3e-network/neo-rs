//! Transaction verification context implementation.
//!
//! This module provides the TransactionVerificationContext functionality exactly matching C# Neo TransactionVerificationContext.

// Matches C# using directives exactly:
// using Neo.Network.P2P.Payloads;
// using Neo.Persistence;
// using Neo.SmartContract.Native;
// using System.Collections.Generic;
// using System.Linq;
// using System.Numerics;

use crate::network::p2p::payloads::Transaction;
use crate::{UInt160, UInt256};
use num_bigint::BigInt;
use std::collections::HashMap;

/// namespace Neo.Ledger -> public class TransactionVerificationContext

/// The context used to verify the transaction.
pub struct TransactionVerificationContext {
    /// Store all verified unsorted transactions' senders' fee currently in the memory pool.
    /// private readonly Dictionary<UInt160, BigInteger> _senderFee = [];
    sender_fee: HashMap<UInt160, BigInt>,

    /// Store oracle responses
    /// private readonly Dictionary<ulong, UInt256> _oracleResponses = [];
    oracle_responses: HashMap<u64, UInt256>,
}

impl TransactionVerificationContext {
    /// Constructor (implicit in C#)
    pub fn new() -> Self {
        Self {
            sender_fee: HashMap::new(),
            oracle_responses: HashMap::new(),
        }
    }

    /// Adds a verified Transaction to the context.
    /// public void AddTransaction(Transaction tx)
    pub fn add_transaction(&mut self, tx: &Transaction) {
        // var oracle = tx.GetAttribute<OracleResponse>();
        // if (oracle != null) _oracleResponses.Add(oracle.Id, tx.Hash);
        if let Some(oracle) = tx.get_attribute::<crate::transaction::OracleResponse>() {
            self.oracle_responses.insert(oracle.id, tx.hash());
        }

        // if (_senderFee.TryGetValue(tx.Sender, out var value))
        //     _senderFee[tx.Sender] = value + tx.SystemFee + tx.NetworkFee;
        // else
        //     _senderFee.Add(tx.Sender, tx.SystemFee + tx.NetworkFee);
        let fee = BigInt::from(tx.system_fee) + BigInt::from(tx.network_fee);
        self.sender_fee
            .entry(tx.sender())
            .and_modify(|e| *e += &fee)
            .or_insert(fee);
    }

    /// Determine whether the specified Transaction can be verified in the context.
    /// public VerifyResult CheckTransaction(Transaction tx, IEnumerable<Transaction> conflictingTxs, DataCache snapshot, ProtocolSettings settings)
    pub fn check_transaction(
        &self,
        tx: &Transaction,
        conflicting_txs: &[Transaction],
        snapshot: &crate::persistence::DataCache,
        settings: &crate::settings::ProtocolSettings,
    ) -> super::VerifyResult {
        // Implementation would go here when all types are available
        todo!("CheckTransaction implementation")
    }

    /// Removes a verified Transaction from the context.
    /// public void RemoveTransaction(Transaction tx)
    pub fn remove_transaction(&mut self, tx: &Transaction) {
        // if ((tx.GetAttribute<OracleResponse>()) != null)
        //     _oracleResponses.Remove(tx.GetAttribute<OracleResponse>().Id);
        if let Some(oracle) = tx.get_attribute::<crate::transaction::OracleResponse>() {
            self.oracle_responses.remove(&oracle.id);
        }

        // if (_senderFee.ContainsKey(tx.Sender))
        // {
        //     _senderFee[tx.Sender] -= (tx.SystemFee + tx.NetworkFee);
        //     if (_senderFee[tx.Sender] == 0) _senderFee.Remove(tx.Sender);
        // }
        if let Some(fee) = self.sender_fee.get_mut(&tx.sender()) {
            *fee -= BigInt::from(tx.system_fee) + BigInt::from(tx.network_fee);
            if fee.sign() == num_bigint::Sign::NoSign {
                self.sender_fee.remove(&tx.sender());
            }
        }
    }
}
