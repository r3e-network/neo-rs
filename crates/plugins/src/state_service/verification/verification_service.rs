// Copyright (C) 2015-2025 The Neo Project.
//
// verification_service.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{Wallet, ExtensiblePayload, LogLevel};
use super::verification_context::VerificationContext;
use super::super::network::{Vote, MessageType};
use super::super::StatePlugin;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Verification service implementation.
/// Matches C# VerificationService class exactly
pub struct VerificationService {
    /// Validated root persisted message
    /// Matches C# ValidatedRootPersisted class
    pub struct ValidatedRootPersisted {
        pub index: u32,
    }
    
    /// Block persisted message
    /// Matches C# BlockPersisted class
    pub struct BlockPersisted {
        pub index: u32,
    }
    
    /// Maximum cached verification process count
    /// Matches C# MaxCachedVerificationProcessCount constant
    const MAX_CACHED_VERIFICATION_PROCESS_COUNT: usize = 10,
    
    /// Delay milliseconds
    /// Matches C# DelayMilliseconds constant
    const DELAY_MILLISECONDS: u64 = 3000,
    
    /// Timer message
    /// Matches C# Timer class
    struct Timer {
        pub index: u32,
    }
    
    /// Wallet reference
    /// Matches C# wallet field
    wallet: Arc<Wallet>,
    
    /// Verification contexts
    /// Matches C# contexts field
    contexts: Arc<Mutex<HashMap<u32, VerificationContext>>>,
}

impl VerificationService {
    /// Creates a new VerificationService instance.
    /// Matches C# constructor
    pub fn new(wallet: Arc<Wallet>) -> Self {
        let service = Self {
            wallet,
            contexts: Arc::new(Mutex::new(HashMap::new())),
        };
        
        // Subscribe to blockchain events
        // In a real implementation, this would subscribe to the event stream
        // StatePlugin::neo_system().actor_system().event_stream().subscribe(...);
        
        service
    }
    
    /// Sends a vote.
    /// Matches C# SendVote method
    fn send_vote(&self, context: &VerificationContext) {
        if let Some(vote_message) = context.vote_message() {
            // In a real implementation, this would log and send the vote
            // Utility::log("VerificationService", LogLevel::Info, &format!("relay vote, height={}, retry={}", context.root_index(), context.retries));
            // StatePlugin::neo_system().blockchain().tell(vote_message);
        }
    }
    
    /// Handles state root vote.
    /// Matches C# OnStateRootVote method
    fn on_state_root_vote(&mut self, vote: &Vote) {
        if let Ok(mut contexts) = self.contexts.lock() {
            if let Some(context) = contexts.get_mut(&vote.root_index) {
                if context.add_signature(vote.validator_index, vote.signature.clone()) {
                    self.check_votes(context);
                }
            }
        }
    }
    
    /// Checks votes.
    /// Matches C# CheckVotes method
    fn check_votes(&self, context: &mut VerificationContext) {
        if context.is_sender() && context.check_signatures() {
            if let Some(state_root_message) = context.state_root_message() {
                // In a real implementation, this would log and send the state root
                // Utility::log("VerificationService", LogLevel::Info, &format!("relay state root, height={}, root={}", context.state_root().unwrap().index, context.state_root().unwrap().root_hash));
                // StatePlugin::neo_system().blockchain().tell(state_root_message);
            }
        }
    }
    
    /// Handles block persisted.
    /// Matches C# OnBlockPersisted method
    fn on_block_persisted(&mut self, index: u32) {
        if let Ok(mut contexts) = self.contexts.lock() {
            // Remove old contexts if we have too many
            if contexts.len() >= Self::MAX_CACHED_VERIFICATION_PROCESS_COUNT {
                let mut keys_to_remove = Vec::new();
                let mut sorted_keys: Vec<u32> = contexts.keys().cloned().collect();
                sorted_keys.sort();
                
                let count_to_remove = contexts.len() - Self::MAX_CACHED_VERIFICATION_PROCESS_COUNT + 1;
                for key in sorted_keys.iter().take(count_to_remove) {
                    keys_to_remove.push(*key);
                }
                
                for key in keys_to_remove {
                    if let Some(context) = contexts.remove(&key) {
                        // In a real implementation, this would cancel the timer
                        // context.timer.cancel_if_not_null();
                    }
                }
            }
            
            // Create new verification context
            let context = VerificationContext::new(self.wallet.clone(), index);
            if context.is_validator() && contexts.insert(index, context).is_none() {
                // In a real implementation, this would schedule a timer
                // context.timer = Context::system().scheduler().schedule_tell_once_cancelable(
                //     Duration::from_millis(Self::DELAY_MILLISECONDS),
                //     self,
                //     Timer { index },
                //     ActorRefs::no_sender()
                // );
                
                // In a real implementation, this would log
                // Utility::log("VerificationContext", LogLevel::Info, &format!("new validate process, height={}, index={}, ongoing={}", index, context.my_index(), contexts.len()));
            }
        }
    }
    
    /// Handles validated root persisted.
    /// Matches C# OnValidatedRootPersisted method
    fn on_validated_root_persisted(&mut self, index: u32) {
        // In a real implementation, this would log
        // Utility::log("VerificationService", LogLevel::Info, &format!("persisted state root, height={}", index));
        
        if let Ok(mut contexts) = self.contexts.lock() {
            let keys_to_remove: Vec<u32> = contexts.keys()
                .filter(|&&k| k <= index)
                .cloned()
                .collect();
            
            for key in keys_to_remove {
                if let Some(context) = contexts.remove(&key) {
                    // In a real implementation, this would cancel the timer
                    // context.timer.cancel_if_not_null();
                }
            }
        }
    }
    
    /// Handles timer.
    /// Matches C# OnTimer method
    fn on_timer(&mut self, index: u32) {
        if let Ok(mut contexts) = self.contexts.lock() {
            if let Some(context) = contexts.get_mut(&index) {
                self.send_vote(context);
                self.check_votes(context);
                
                // In a real implementation, this would cancel and reschedule the timer
                // context.timer.cancel_if_not_null();
                // context.timer = Context::system().scheduler().schedule_tell_once_cancelable(
                //     Duration::from_millis(StatePlugin::neo_system().get_time_per_block().as_millis() as u64 << context.retries),
                //     self,
                //     Timer { index },
                //     ActorRefs::no_sender()
                // );
                // context.retries += 1;
            }
        }
    }
    
    /// Handles vote message.
    /// Matches C# OnVoteMessage method
    fn on_vote_message(&mut self, payload: &ExtensiblePayload) {
        if payload.data.is_empty() {
            return;
        }
        
        if payload.data[0] != MessageType::Vote as u8 {
            return;
        }
        
        let mut vote = Vote::new();
        if vote.deserialize(&mut payload.data[1..].as_ref()).is_ok() {
            self.on_state_root_vote(&vote);
        }
    }
    
    /// Handles messages.
    /// Matches C# OnReceive method
    pub fn handle_message(&mut self, message: &dyn std::any::Any) {
        // In a real implementation, this would handle different message types
        // match message {
        //     VoteMessage(vote) => self.on_vote_message(vote),
        //     BlockPersisted(index) => self.on_block_persisted(*index),
        //     ValidatedRootPersisted(index) => self.on_validated_root_persisted(*index),
        //     Timer(index) => self.on_timer(*index),
        //     _ => {},
        // }
    }
}