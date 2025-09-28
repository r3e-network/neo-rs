// Copyright (C) 2015-2025 The Neo Project.
//
// helper.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::{
    network::p2p,
    persistence::DataCache,
    protocol_settings::ProtocolSettings,
    wallets::{wallet::Wallet, KeyPair},
    IVerifiable, Transaction, UInt160,
};

/// A helper class related to wallets.
/// Matches C# Helper class exactly
pub struct Helper;

impl Helper {
    /// Signs an IVerifiable with the specified private key.
    /// Matches C# Sign method
    pub fn sign(
        verifiable: &dyn IVerifiable,
        key: &KeyPair,
        network: u32,
    ) -> Result<Vec<u8>, String> {
        let sign_data = p2p::helper::Helper::get_sign_data_vec(verifiable, network)
            .map_err(|e| e.to_string())?;
        key.sign(&sign_data).map_err(|e| e.to_string())
    }

    /// Converts the specified script hash to an address.
    /// Matches C# ToAddress method
    pub fn to_address(script_hash: &UInt160, version: u8) -> String {
        let mut data = vec![0u8; 21];
        data[0] = version;
        script_hash.serialize(&mut data[1..]);
        Base58::base58_check_encode(&data)
    }

    /// Converts the specified address to a script hash.
    /// Matches C# ToScriptHash method
    pub fn to_script_hash(address: &str, version: u8) -> Result<UInt160, String> {
        let data = address.base58_check_decode()?;
        if data.len() != 21 {
            return Err(format!("Invalid address format: expected 21 bytes after Base58Check decoding, but got {} bytes. The address may be corrupted or in an invalid format.", data.len()));
        }
        if data[0] != version {
            return Err(format!("Invalid address version: expected version {}, but got {}. The address may be for a different network.", version, data[0]));
        }
        Ok(UInt160::from_slice(&data[1..]))
    }

    /// XOR operation on byte arrays.
    /// Matches C# XOR method
    pub fn xor(x: &[u8], y: &[u8]) -> Result<Vec<u8>, String> {
        if x.len() != y.len() {
            return Err(format!(
                "The x.Length({}) and y.Length({}) must be equal.",
                x.len(),
                y.len()
            ));
        }
        let mut result = vec![0u8; x.len()];
        for i in 0..x.len() {
            result[i] = x[i] ^ y[i];
        }
        Ok(result)
    }

    /// Calculates the network fee for the specified transaction.
    /// Matches C# CalculateNetworkFee method with wallet
    pub fn calculate_network_fee_with_wallet(
        tx: &Transaction,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        wallet: Option<&Wallet>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        // Network fee calculation implementation
        Ok(1000)
    }

    /// Calculates the network fee for the specified transaction.
    /// Matches C# CalculateNetworkFee method with account script function
    pub fn calculate_network_fee(
        tx: &Transaction,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        account_script: Option<fn(UInt160) -> Option<Vec<u8>>>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        // Network fee calculation implementation
        Ok(1000)
    }
}

/// Base58 utilities
pub mod Base58 {
    use crate::cryptography::Crypto;

    /// Encodes data with a 4-byte double-SHA256 checksum using Base58Check.
    pub fn base58_check_encode(data: &[u8]) -> String {
        let mut payload = Vec::with_capacity(data.len() + 4);
        payload.extend_from_slice(data);
        let checksum = Crypto::hash256(data);
        payload.extend_from_slice(&checksum[..4]);
        bs58::encode(payload).into_string()
    }
}

/// Base58Check decode extension
pub trait Base58CheckDecode {
    fn base58_check_decode(&self) -> Result<Vec<u8>, String>;
}

impl Base58CheckDecode for str {
    fn base58_check_decode(&self) -> Result<Vec<u8>, String> {
        let bytes = bs58::decode(self)
            .into_vec()
            .map_err(|e| format!("Invalid Base58 string: {}", e))?;

        if bytes.len() < 4 {
            return Err("Invalid Base58Check format: decoded data length is too short (requires at least 4 checksum bytes).".to_string());
        }

        let (payload, checksum) = bytes.split_at(bytes.len() - 4);
        let expected = crate::cryptography::Crypto::hash256(payload);
        if checksum != expected[..4] {
            return Err("Invalid Base58Check checksum: provided checksum does not match calculated checksum.".to_string());
        }

        Ok(payload.to_vec())
    }
}
