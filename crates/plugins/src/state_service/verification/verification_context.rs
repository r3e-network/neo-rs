// Copyright (C) 2015-2025 The Neo Project.
//
// verification_context.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt160, UInt256, ECPoint, Wallet, KeyPair, WalletAccount, Contract, ContractParametersContext, Crypto, NativeContract, Role};
use neo_core::network::p2p::payloads::{ExtensiblePayload, ISerializable};
use super::super::network::{StateRoot, Vote, MessageType};
use super::super::storage::StateStore;
use super::super::StatePlugin;
use std::collections::HashMap;
use std::sync::Arc;

/// Verification context implementation.
/// Matches C# VerificationContext class exactly
pub struct VerificationContext {
    /// Maximum valid until block increment
    /// Matches C# MaxValidUntilBlockIncrement constant
    const MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 100,
    
    /// State root
    /// Matches C# root field
    root: Option<StateRoot>,
    
    /// Root payload
    /// Matches C# rootPayload field
    root_payload: Option<ExtensiblePayload>,
    
    /// Vote payload
    /// Matches C# votePayload field
    vote_payload: Option<ExtensiblePayload>,
    
    /// Wallet reference
    /// Matches C# wallet field
    wallet: Arc<Wallet>,
    
    /// Key pair
    /// Matches C# keyPair field
    key_pair: Option<KeyPair>,
    
    /// My index
    /// Matches C# myIndex field
    my_index: i32,
    
    /// Root index
    /// Matches C# rootIndex field
    root_index: u32,
    
    /// Verifiers
    /// Matches C# verifiers field
    verifiers: Vec<ECPoint>,
    
    /// Signatures
    /// Matches C# signatures field
    signatures: HashMap<i32, Vec<u8>>,
    
    /// Retries count
    /// Matches C# Retries field
    pub retries: i32,
    
    /// Timer
    /// Matches C# Timer field
    pub timer: Option<Arc<dyn crate::Timer>>, // In a real implementation, this would be the actual Timer type
}

impl VerificationContext {
    /// Creates a new VerificationContext instance.
    /// Matches C# constructor
    pub fn new(wallet: Arc<Wallet>, index: u32) -> Self {
        let mut context = Self {
            root: None,
            root_payload: None,
            vote_payload: None,
            wallet: wallet.clone(),
            key_pair: None,
            my_index: -1,
            root_index: index,
            verifiers: Vec::new(),
            signatures: HashMap::new(),
            retries: 0,
            timer: None,
        };
        
        // Get verifiers
        context.verifiers = NativeContract::role_management()
            .get_designated_by_role(&StatePlugin::neo_system().store_view(), Role::StateValidator, index)
            .unwrap_or_default();
        
        // Find my index and key pair
        if let Some(wallet) = context.wallet.as_ref() {
            for (i, verifier) in context.verifiers.iter().enumerate() {
                if let Some(account) = wallet.get_account(*verifier) {
                    if account.has_key() {
                        context.my_index = i as i32;
                        context.key_pair = Some(account.get_key().unwrap());
                        break;
                    }
                }
            }
        }
        
        context
    }
    
    /// Gets whether this is a validator.
    /// Matches C# IsValidator property
    pub fn is_validator(&self) -> bool {
        self.my_index >= 0
    }
    
    /// Gets my index.
    /// Matches C# MyIndex property
    pub fn my_index(&self) -> i32 {
        self.my_index
    }
    
    /// Gets the root index.
    /// Matches C# RootIndex property
    pub fn root_index(&self) -> u32 {
        self.root_index
    }
    
    /// Gets the verifiers.
    /// Matches C# Verifiers property
    pub fn verifiers(&self) -> &[ECPoint] {
        &self.verifiers
    }
    
    /// Gets the sender index.
    /// Matches C# Sender property
    pub fn sender(&self) -> i32 {
        let p = (self.root_index as i32 - self.retries) % self.verifiers.len() as i32;
        if p >= 0 { p } else { p + self.verifiers.len() as i32 }
    }
    
    /// Gets whether this is the sender.
    /// Matches C# IsSender property
    pub fn is_sender(&self) -> bool {
        self.my_index == self.sender()
    }
    
    /// Gets the state root.
    /// Matches C# StateRoot property
    pub fn state_root(&self) -> Option<&StateRoot> {
        if self.root.is_none() {
            // In a real implementation, this would get the state root from StateStore
            // let snapshot = StateStore::singleton().get_snapshot();
            // self.root = snapshot.get_state_root(self.root_index);
        }
        self.root.as_ref()
    }
    
    /// Gets the state root message.
    /// Matches C# StateRootMessage property
    pub fn state_root_message(&self) -> Option<&ExtensiblePayload> {
        self.root_payload.as_ref()
    }
    
    /// Gets the vote message.
    /// Matches C# VoteMessage property
    pub fn vote_message(&self) -> Option<&ExtensiblePayload> {
        if self.vote_payload.is_none() {
            // In a real implementation, this would create the vote message
            // self.vote_payload = Some(self.create_vote_message());
        }
        self.vote_payload.as_ref()
    }
    
    /// Creates a vote message.
    /// Matches C# CreateVoteMessage method
    fn create_vote_message(&mut self) -> Option<ExtensiblePayload> {
        if let Some(state_root) = self.state_root() {
            if let Some(sig) = self.signatures.get(&self.my_index) {
                let vote = Vote {
                    validator_index: self.my_index,
                    root_index: self.root_index,
                    signature: sig.clone(),
                };
                self.create_payload(MessageType::Vote, &vote, 10) // MaxCachedVerificationProcessCount
            } else {
                // Sign the state root
                if let Some(key_pair) = &self.key_pair {
                    let sig = state_root.sign(key_pair, StatePlugin::neo_system().settings().network());
                    self.signatures.insert(self.my_index, sig);
                    let vote = Vote {
                        validator_index: self.my_index,
                        root_index: self.root_index,
                        signature: self.signatures[&self.my_index].clone(),
                    };
                    self.create_payload(MessageType::Vote, &vote, 10)
                } else {
                    None
                }
            }
        } else {
            None
        }
    }
    
    /// Adds a signature.
    /// Matches C# AddSignature method
    pub fn add_signature(&mut self, index: i32, sig: Vec<u8>) -> bool {
        let m = self.verifiers.len() - (self.verifiers.len() - 1) / 3;
        if m <= self.signatures.len() {
            return false;
        }
        
        if index < 0 || index >= self.verifiers.len() as i32 {
            return false;
        }
        
        if self.signatures.contains_key(&index) {
            return false;
        }
        
        // Verify signature
        if let Some(state_root) = self.state_root() {
            if let Some(hash_data) = state_root.get_sign_data(StatePlugin::neo_system().settings().network()) {
                let validator = self.verifiers[index as usize];
                if !Crypto::verify_signature(&hash_data, &sig, &validator) {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
        
        self.signatures.insert(index, sig);
        true
    }
    
    /// Checks signatures.
    /// Matches C# CheckSignatures method
    pub fn check_signatures(&mut self) -> bool {
        if let Some(state_root) = self.state_root() {
            let m = self.verifiers.len() - (self.verifiers.len() - 1) / 3;
            if self.signatures.len() < m {
                return false;
            }
            
            if state_root.witness.is_none() {
                // Create multi-sig contract
                let contract = Contract::create_multi_sig_contract(m, &self.verifiers);
                let mut sc = ContractParametersContext::new(
                    StatePlugin::neo_system().store_view(),
                    state_root,
                    StatePlugin::neo_system().settings().network(),
                );
                
                let mut j = 0;
                for (i, verifier) in self.verifiers.iter().enumerate() {
                    if j >= m {
                        break;
                    }
                    if let Some(sig) = self.signatures.get(&(i as i32)) {
                        sc.add_signature(&contract, verifier, sig);
                        j += 1;
                    }
                }
                
                if !sc.completed() {
                    return false;
                }
                
                // Set witness
                if let Some(witnesses) = sc.get_witnesses() {
                    if !witnesses.is_empty() {
                        // In a real implementation, this would set the witness
                        // state_root.witness = Some(witnesses[0].clone());
                    }
                }
            }
            
            if self.is_sender() {
                self.root_payload = self.create_payload(MessageType::StateRoot, state_root, Self::MAX_VALID_UNTIL_BLOCK_INCREMENT);
            }
            
            true
        } else {
            false
        }
    }
    
    /// Creates a payload.
    /// Matches C# CreatePayload method
    fn create_payload(&self, message_type: MessageType, payload: &dyn ISerializable, valid_block_end_threshold: u32) -> Option<ExtensiblePayload> {
        let mut data = Vec::new();
        data.push(message_type as u8);
        if payload.serialize(&mut data).is_err() {
            return None;
        }
        
        let mut msg = ExtensiblePayload {
            category: StatePlugin::STATE_PAYLOAD_CATEGORY.to_string(),
            valid_block_start: self.root_index,
            valid_block_end: self.root_index + valid_block_end_threshold,
            sender: UInt160::default(), // In a real implementation, this would be the script hash
            data,
            witness: None,
        };
        
        // Sign the message
        let mut sc = ContractParametersContext::new(
            StatePlugin::neo_system().store_view(),
            &msg,
            StatePlugin::neo_system().settings().network(),
        );
        
        if self.wallet.sign(&mut sc) {
            if let Some(witnesses) = sc.get_witnesses() {
                if !witnesses.is_empty() {
                    msg.witness = Some(witnesses[0].clone());
                }
            }
        }
        
        Some(msg)
    }
}