//! Session construction and initial script execution.
//!
//! `Session::new` builds the C#-compatible invocation context: storage snapshot,
//! optional transaction container, dummy persisting block, native provider, and
//! executed `ApplicationEngine`. Keeping that workflow here leaves the session
//! root focused on retained state and iterator lifecycle methods.

use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_manifest::CallFlags;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_payloads::witness::Witness;
use neo_primitives::{TriggerType, Verifiable};
use neo_runtime::{ConfigProvider, StoreProvider};
use parking_lot::Mutex;
use rand::random;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::server::diagnostic::Diagnostic;
use crate::server::ledger_queries;

use super::Session;
use super::dummy_block::create_dummy_block;
use super::native_provider::{NativeSessionProvider, SessionNativeProvider};

impl Session {
    /// Create and execute a new invocation session.
    ///
    /// The session owns the executed engine, a storage snapshot, any diagnostic
    /// output, and later any VM iterators exposed by the invocation result.
    // Rationale: invocation sessions are the RPC execution composition seam and
    // must receive providers, script, signers, witnesses, gas, and diagnostics explicitly.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store_provider: Arc<dyn StoreProvider>,
        config_provider: Arc<dyn ConfigProvider>,
        native_contract_provider: Arc<dyn NativeContractProvider>,
        script: Vec<u8>,
        signers: Option<Vec<Signer>>,
        witnesses: Option<Vec<Witness>>,
        gas_limit: i64,
        diagnostic: Option<Diagnostic>,
    ) -> CoreResult<Self> {
        let store_cache = store_provider.store_cache();
        let snapshot_cache = Arc::new(store_cache.data_cache().clone());

        let settings = config_provider.settings();

        // C# `NeoSystemExtensions.GetMaxValidUntilBlockIncrement(snapshot,
        // settings)`: before HF_Echidna the static protocol setting, from
        // HF_Echidna onward the Policy storage value (falling back to the
        // setting when the key is not yet initialized). The static
        // `ConfigProvider::max_valid_until_block_increment()` is only correct
        // pre-Echidna, so read the Policy-aware value from the snapshot.
        let session_native_provider =
            NativeSessionProvider::new(Arc::clone(&native_contract_provider));
        let max_valid_until_block_increment = session_native_provider
            .max_valid_until_block_increment(store_cache.data_cache(), settings.as_ref())
            .unwrap_or_else(|_| config_provider.max_valid_until_block_increment());

        let tx_container = signers.as_ref().map(|signer_list| {
            let mut tx = Transaction::new();
            tx.set_version(0);
            tx.set_nonce(random());
            let valid_until = ledger_queries::current_index(store_cache.data_cache())
                .unwrap_or(0)
                .saturating_add(max_valid_until_block_increment);
            tx.set_valid_until_block(valid_until);
            tx.set_signers(signer_list.clone());
            tx.set_attributes(Vec::<TransactionAttribute>::new());
            tx.set_script(script.clone());
            if let Some(ws) = &witnesses {
                tx.set_witnesses(ws.clone());
            } else {
                tx.set_witnesses(vec![Witness::new(); signer_list.len()]);
            }
            Arc::new(tx) as Arc<dyn Verifiable>
        });

        // C# `ApplicationEngine.Run` (invoked by the RPC invoke* methods) has no
        // persisting block, so it synthesizes one via
        // `ApplicationEngine.CreateDummyBlock(snapshot, settings)`. Without it,
        // `System.Runtime.GetTime` faults and `LedgerContract.CurrentIndex`-style
        // reads see `height` instead of the `height + 1` a real persisting block
        // would give. Build the same dummy block so stateless invoke *results*
        // (GetTime, currentindex) match C# field-for-field.
        let persisting_block = create_dummy_block(
            store_cache.data_cache(),
            settings.as_ref(),
            &session_native_provider,
        )
        .map(Arc::new);

        let diagnostic_box = diagnostic
            .clone()
            .map(|diag| Box::new(diag) as Box<dyn neo_execution::diagnostic::Diagnostic>);

        let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            tx_container,
            Arc::clone(&snapshot_cache),
            persisting_block,
            settings.as_ref().clone(),
            gas_limit,
            diagnostic_box,
            Some(native_contract_provider),
        )
        .map_err(|err| CoreError::other(err.to_string()))?;

        engine
            .load_script(script.clone(), CallFlags::ALL, None)
            .map_err(|err| CoreError::other(err.to_string()))?;
        engine.execute_allow_fault();

        Ok(Self {
            script,
            snapshot: store_cache,
            engine: Mutex::new(engine),
            diagnostic: Mutex::new(diagnostic),
            iterators: Mutex::new(HashMap::new()),
            iterator_lookup: Mutex::new(HashMap::new()),
            start_time: Mutex::new(Instant::now()),
        })
    }
}
