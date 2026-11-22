use super::*;
impl CommittingHandler for ApplicationLogsService {
    fn run_during_fast_sync(&self) -> bool {
        // `.acc` import enables fast sync. Application logs are the behaviour
        // oracle for that import, so this handler must still run.
        true
    }

    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &ExecutionBlock,
        _snapshot: &PersistenceDataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            self.start_batch();

            let block_hash = block.hash();
            // A unit can span several blocks (a coalesced run of empty blocks),
            // and the callbacks arrive in ascending height, so the newest note
            // wins and the unit reports the height it actually reaches.
            self.note_block_boundary(block.index(), block_hash);
            let block_log = self.block_log_json(&block_hash, application_executed_list);
            self.write_log(Self::PREFIX_BLOCK, &block_hash, block_log)?;

            for exec in application_executed_list {
                let Some(tx) = exec.transaction.as_ref() else {
                    continue;
                };
                let tx_hash = tx.hash();
                let tx_log = self.transaction_log_json(&tx_hash, exec);
                self.write_log(Self::PREFIX_TX, &tx_hash, tx_log)?;
            }
            Ok(())
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err, "committing"),
            Err(payload) => self.handle_panic(payload, "committing"),
        }
    }
}

impl CommittedHandler for ApplicationLogsService {
    fn blockchain_committed_handler(&self, system: &dyn Any, _block: &ExecutionBlock) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| self.commit_batch()));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err, "committed"),
            Err(payload) => self.handle_panic(payload, "committed"),
        }
    }
}
