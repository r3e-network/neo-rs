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

use crate::network::p2p::payloads::i_verifiable::IVerifiable as PayloadIVerifiable;
use crate::IVerifiable as CoreIVerifiable;
use crate::{
    network::p2p,
    persistence::DataCache,
    protocol_settings::ProtocolSettings,
    smart_contract::helper::Helper as ContractHelper,
    smart_contract::native::PolicyContract,
    wallets::{wallet::Wallet, KeyPair},
    Transaction, UInt160,
};
use neo_vm::op_code::OpCode;

/// A helper class related to wallets.
/// Matches C# Helper class exactly
pub struct Helper;

impl Helper {
    /// Signs an IVerifiable with the specified private key.
    /// Matches C# Sign method
    pub fn sign(
        verifiable: &dyn CoreIVerifiable,
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
        let mut data = Vec::with_capacity(21);
        data.push(version);
        data.extend_from_slice(&script_hash.to_array());
        base58::base58_check_encode(&data)
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
        UInt160::from_bytes(&data[1..]).map_err(|e| e.to_string())
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
        wallet: Option<&dyn Wallet>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        match wallet {
            Some(wallet) => {
                let resolver: Box<AccountScriptResolver<'_>> = Box::new(move |hash: &UInt160| {
                    wallet
                        .get_account(hash)
                        .and_then(|account| account.contract().map(|c| c.script.clone()))
                });
                calculate_network_fee_impl(
                    tx,
                    snapshot,
                    settings,
                    Some(resolver.as_ref()),
                    max_execution_cost,
                )
            }
            None => calculate_network_fee_impl(tx, snapshot, settings, None, max_execution_cost),
        }
    }

    /// Calculates the network fee for the specified transaction.
    /// Matches C# CalculateNetworkFee method with account script function
    pub fn calculate_network_fee(
        tx: &Transaction,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        account_script: Option<Box<AccountScriptResolver<'_>>>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        calculate_network_fee_impl(
            tx,
            snapshot,
            settings,
            account_script.as_deref(),
            max_execution_cost,
        )
    }
}

fn calculate_network_fee_impl(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    account_script: Option<&AccountScriptResolver<'_>>,
    _max_execution_cost: i64,
) -> Result<i64, String> {
    let mut hashes = PayloadIVerifiable::get_script_hashes_for_verifying(tx, snapshot);
    hashes.sort();
    hashes.dedup();

    let exec_fee_factor = PolicyContract::DEFAULT_EXEC_FEE_FACTOR as i64;
    let fee_per_byte = PolicyContract::DEFAULT_FEE_PER_BYTE as i64;

    let mut size: i64 = 0;
    let mut network_fee: i64 = 0;

    for (index, hash) in hashes.iter().enumerate() {
        let witness_script = account_script
            .and_then(|resolver| resolver(hash))
            .or_else(|| {
                tx.witnesses()
                    .get(index)
                    .map(|w| w.verification_script.clone())
            });

        if witness_script
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            return Err(format!(
                "The smart contract or address {} ({}) is not found. If this is your wallet address and you want to sign a transaction with it, make sure you have opened this wallet.",
                hex::encode(hash.to_array()),
                Helper::to_address(hash, settings.address_version)
            ));
        }

        let witness_script = witness_script.expect("script presence checked");

        if ContractHelper::is_signature_contract(&witness_script) {
            size += 67 + var_size_with_payload(witness_script.len());
            network_fee += exec_fee_factor * ContractHelper::signature_contract_cost();
        } else if let Some((m, n)) = parse_multi_sig_contract(&witness_script) {
            let invocation_len = 66 * m as i64;
            size += var_size_with_payload(invocation_len as usize) + invocation_len;
            size += var_size_with_payload(witness_script.len());
            network_fee +=
                exec_fee_factor * ContractHelper::multi_signature_contract_cost(m as i32, n as i32);
        } else {
            return Err(format!(
                "Contract-based verification for script hash {} is not yet supported in this build.",
                hex::encode(hash.to_array())
            ));
        }
    }

    network_fee += size * fee_per_byte;
    for attribute in tx.attributes() {
        network_fee += attribute.calculate_network_fee(snapshot, tx);
    }

    Ok(network_fee)
}

fn var_size_prefix(len: usize) -> i64 {
    if len < 0xFD {
        1
    } else if len <= 0xFFFF {
        3
    } else if len <= 0xFFFF_FFFF {
        5
    } else {
        9
    }
}

fn var_size_with_payload(len: usize) -> i64 {
    var_size_prefix(len) + len as i64
}

fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, usize)> {
    if script.len() < 43 {
        return None;
    }

    let first = OpCode::try_from(script[0]).ok()?;
    let first_byte = first as u8;
    if !((OpCode::PUSH1 as u8)..=(OpCode::PUSH16 as u8)).contains(&first_byte) {
        return None;
    }
    let m = (first as u8 - OpCode::PUSH0 as u8) as usize;

    let mut offset = 1;
    let mut n = 0usize;
    while offset < script.len() {
        if script[offset] != OpCode::PUSHDATA1 as u8 {
            break;
        }
        if offset + 2 >= script.len() {
            return None;
        }
        let key_len = script[offset + 1] as usize;
        if key_len != 33 || offset + 2 + key_len > script.len() {
            return None;
        }
        offset += 2 + key_len;
        n += 1;
    }

    if n == 0 || offset >= script.len() {
        return None;
    }

    let push_n = OpCode::try_from(script[offset]).ok()?;
    let opcode_value = push_n as u8;
    if !((OpCode::PUSH1 as u8)..=(OpCode::PUSH16 as u8)).contains(&opcode_value) {
        return None;
    }
    if (push_n as u8 - OpCode::PUSH0 as u8) as usize != n {
        return None;
    }
    offset += 1;

    if offset + 5 != script.len() {
        return None;
    }
    if script[offset] != OpCode::SYSCALL as u8 {
        return None;
    }

    Some((m, n))
}

/// Base58 utilities
pub mod base58 {
    use crate::cryptography::crypto_utils::Base58;

    /// Encodes data with a 4-byte double-SHA256 checksum using Base58Check.
    pub fn base58_check_encode(data: &[u8]) -> String {
        Base58::encode_check(data)
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
        let expected = crate::cryptography::crypto_utils::NeoHash::hash256(payload);
        if checksum != &expected[..4] {
            return Err("Invalid Base58Check checksum: provided checksum does not match calculated checksum.".to_string());
        }

        Ok(payload.to_vec())
    }
}
pub type AccountScriptResolver<'a> = dyn Fn(&UInt160) -> Option<Vec<u8>> + 'a;
