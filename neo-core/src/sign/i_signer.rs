// Copyright (C) 2015-2025 The Neo Project.
//
// i_signer.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::cryptography::ECPoint;
use crate::network::p2p::payloads::{Block, ExtensiblePayload, Witness};
use crate::persistence::DataCache;

/// Represents a signer that can sign messages.
pub trait ISigner: Send + Sync {
    /// Signs the ExtensiblePayload with the wallet.
    ///
    /// # Arguments
    /// * `payload` - The ExtensiblePayload to be used.
    /// * `snapshot` - The snapshot.
    /// * `network` - The network.
    ///
    /// # Returns
    /// The witness.
    ///
    /// # Errors
    /// Returns error when the payload is null.
    fn sign_extensible_payload(
        &self,
        payload: &ExtensiblePayload,
        snapshot: &DataCache,
        network: u32,
    ) -> Result<Witness, SignException>;

    /// Signs the specified data with the corresponding private key of the specified public key.
    ///
    /// # Arguments
    /// * `block` - The block to sign.
    /// * `public_key` - The public key.
    /// * `network` - The network.
    ///
    /// # Returns
    /// The signature.
    ///
    /// # Errors
    /// Returns error when the block or public key is null, or when the account
    /// is not found or not signable, or the network is not matching.
    fn sign_block(
        &self,
        block: &Block,
        public_key: &ECPoint,
        network: u32,
    ) -> Result<Vec<u8>, SignException>;

    /// Checks if the wallet contains an account (has private key and is not locked)
    /// with the specified public key.
    /// If the wallet has the public key but not the private key or the account is locked,
    /// it will return false.
    ///
    /// # Arguments
    /// * `public_key` - The public key.
    ///
    /// # Returns
    /// `true` if the wallet contains the specified public key and the corresponding
    /// unlocked private key; otherwise, `false`.
    fn contains_signable(&self, public_key: &ECPoint) -> bool;
}

use super::sign_exception::SignException;
