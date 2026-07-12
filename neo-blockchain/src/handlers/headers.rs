use std::sync::Arc;

use neo_payloads::header::Header;

use crate::command::HeaderValidationOutcome;
use crate::ledger_provider::{BlockProvider, ChainTipProvider};
use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::ValidateHeaders`] batch.
    ///
    /// C# `Blockchain.OnNewHeaders`: each header must chain onto the previous
    /// one and verify (`Header.Verify(settings, snapshot, headerCache)`) before
    /// it is cached; verification failure stops the batch (the C# `break`),
    /// keeping the valid prefix. The anchor for the first header is the last
    /// cached header, or the ledger tip when the cache is empty.
    pub(crate) fn handle_headers(&self, headers: Vec<Header>) -> HeaderValidationOutcome {
        let snapshot = self.system.store_snapshot();
        let settings = self.system.settings();
        let native_contract_provider = self.system.native_contract_provider();

        // C# verification anchor: HeaderCache.Last, else the ledger tip block.
        let mut prev: Option<Header> = self.header_cache.last();
        if prev.is_none()
            && let Some(snap) = &snapshot
        {
            let provider = self.system.ledger_provider(snap.as_ref());
            if let Ok(tip_hash) = provider.current_hash() {
                prev = provider.header_by_hash(&tip_hash).ok().flatten();
            }
        }

        let mut header_height = prev
            .as_ref()
            .map(|h| h.index())
            .unwrap_or_else(|| self.ledger.current_height());
        let mut accepted = 0usize;

        for header in headers.into_iter() {
            let index = header.index();
            if index <= header_height {
                let known_hash = self.header_cache.hash_at(index).or_else(|| {
                    snapshot.as_ref().and_then(|snap| {
                        self.system
                            .ledger_provider(snap.as_ref())
                            .block_hash_by_index(index)
                            .ok()
                            .flatten()
                    })
                });
                match known_hash {
                    Some(hash) if hash == header.hash() => {
                        accepted += 1;
                        continue;
                    }
                    Some(_) => break,
                    None => continue,
                }
            }

            if index != header_height + 1 {
                break;
            }

            // C# Header.Verify(settings, snapshot, headerCache): primary index in
            // range, links onto the anchor, timestamp strictly increases, and the
            // consensus witness satisfies the anchor's NextConsensus (3-GAS cap).
            // Skipped only when no store snapshot is available (no anchor to
            // verify against, e.g. header-only unit fixtures).
            if let (Some(snap), Some(prev_header)) = (&snapshot, &prev) {
                let Some(provider) = &native_contract_provider else {
                    break;
                };
                if i32::from(header.primary_index()) >= settings.validators_count {
                    break;
                }
                if header.prev_hash() != &prev_header.hash() {
                    break;
                }
                if header.timestamp() <= prev_header.timestamp() {
                    break;
                }
                let next_consensus = *prev_header.next_consensus();
                if neo_execution::Helper::verify_witness_with_native_provider(
                    &header,
                    settings.as_ref(),
                    snap,
                    &next_consensus,
                    &header.witness,
                    300_000_000,
                    Arc::clone(provider),
                )
                .is_err()
                {
                    break;
                }
            }

            if !self.header_cache.add(header.clone()) {
                break;
            }

            accepted += 1;
            header_height = index;
            prev = Some(header);
        }

        HeaderValidationOutcome::new(accepted, prev)
    }
}
