// Copyright (C) 2015-2024 The Neo Project.
//
// transaction_router.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::network::p2p::payloads::Transaction;
use neo_core::ledger::VerifyResult;
use neo_core::system::NeoSystem;
use std::sync::Arc;

pub mod ledger {
    use super::*;

    pub struct TransactionRouter {
        system: Arc<NeoSystem>,
    }

    pub struct Preverify {
        transaction: Transaction,
        relay: bool,
    }

    pub struct PreverifyCompleted {
        transaction: Transaction,
        relay: bool,
        result: VerifyResult,
    }

    impl TransactionRouter {
        pub fn new(system: Arc<NeoSystem>) -> Self {
            Self { system }
        }

        pub fn on_receive(&self, message: Preverify) -> Option<PreverifyCompleted> {
            let verify_result = message.transaction.verify_state_independent(&self.system.settings);
            Some(PreverifyCompleted {
                transaction: message.transaction,
                relay: message.relay,
                result: verify_result,
            })
        }

        pub fn props(system: Arc<NeoSystem>) -> Arc<TransactionRouter> {
            Arc::new(TransactionRouter::new(system))
        }
    }
}
