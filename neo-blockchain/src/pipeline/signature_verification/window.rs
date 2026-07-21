//! Ordered, bounded look-ahead for block-header signature preverification.

use std::borrow::Borrow;
use std::collections::VecDeque;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_payloads::Block;

use super::{
    HeaderSignaturePreverificationTicket, SignatureVerificationCancellation,
    SignatureVerificationPool, SignatureVerificationSubmitError,
};

struct PendingHeaderVerification {
    index: u32,
    hash: neo_primitives::UInt256,
    ticket: HeaderSignaturePreverificationTicket,
}

/// Bounded ordered tickets for the blocks following the canonical import lane.
///
/// Workers may run arbitrarily far ahead inside the configured window, but a
/// ticket is consumed only at its exact input position. Any discontinuity
/// permanently disables the window for the rest of the batch and leaves the
/// caller on the synchronous verifier.
pub(crate) struct OrderedHeaderVerificationWindow {
    active: bool,
    pending: VecDeque<PendingHeaderVerification>,
    submitted: usize,
    max_pending: usize,
    cancellation: SignatureVerificationCancellation,
}

impl Default for OrderedHeaderVerificationWindow {
    fn default() -> Self {
        Self {
            active: true,
            pending: VecDeque::new(),
            submitted: 0,
            max_pending: 0,
            cancellation: SignatureVerificationCancellation::default(),
        }
    }
}

impl OrderedHeaderVerificationWindow {
    fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Removes the ticket for the exact current block, if it was prefetched.
    /// A different block at this position invalidates every later identity-bound
    /// ticket and disables speculation for the remainder of the batch.
    pub(crate) fn take_current(
        &mut self,
        block: &Block,
    ) -> Option<HeaderSignaturePreverificationTicket> {
        let pending = self.pending.front()?;
        let Ok(hash) = block.header.try_hash() else {
            self.disable();
            return None;
        };
        if pending.index != block.index() || pending.hash != hash {
            self.disable();
            return None;
        }
        self.pending.pop_front().map(|pending| pending.ticket)
    }

    /// Refills the look-ahead window with blocks after `position`.
    ///
    /// Parent linkage is checked before queue admission. Every result is bound
    /// to the exact header, witness, and protocol settings; canonical NeoVM
    /// verification remains mandatory at the ordered import fence.
    pub(crate) fn fill_after<T>(
        &mut self,
        position: usize,
        blocks: &[T],
        pool: &SignatureVerificationPool,
        settings: Arc<ProtocolSettings>,
    ) where
        T: Borrow<Block>,
    {
        if !self.active {
            return;
        }

        while self.pending.len() < pool.window() {
            let Some(next_position) = position
                .checked_add(1)
                .and_then(|next| next.checked_add(self.pending.len()))
            else {
                self.disable();
                return;
            };
            let Some(next_block) = blocks.get(next_position).map(Borrow::borrow) else {
                return;
            };
            let Some(parent_block) = blocks
                .get(next_position.saturating_sub(1))
                .map(Borrow::borrow)
            else {
                self.disable();
                return;
            };
            let Ok(parent_hash) = parent_block.header.try_hash() else {
                self.disable();
                return;
            };
            if next_block.index() != parent_block.index().saturating_add(1)
                || next_block.header.prev_hash() != &parent_hash
                || next_block.timestamp() <= parent_block.timestamp()
                || i32::from(next_block.primary_index()) >= settings.validators_count
            {
                self.disable();
                return;
            }
            let Ok(hash) = next_block.header.try_hash() else {
                self.disable();
                return;
            };

            match pool.try_submit_header_witness_cancellable(
                next_block.header.clone(),
                Arc::clone(&settings),
                &self.cancellation,
            ) {
                Ok(ticket) => {
                    self.pending.push_back(PendingHeaderVerification {
                        index: next_block.index(),
                        hash,
                        ticket,
                    });
                    self.submitted = self.submitted.saturating_add(1);
                    self.max_pending = self.max_pending.max(self.pending.len());
                }
                Err(SignatureVerificationSubmitError::QueueFull) => return,
                Err(
                    SignatureVerificationSubmitError::Closed
                    | SignatureVerificationSubmitError::InvalidInput(_),
                ) => {
                    self.disable();
                    return;
                }
            }
        }
    }

    /// Discards every speculative ticket and keeps this batch synchronous.
    pub(crate) fn disable(&mut self) {
        self.active = false;
        self.cancel();
        self.pending.clear();
    }

    pub(crate) const fn submitted(&self) -> usize {
        self.submitted
    }

    pub(crate) const fn max_pending(&self) -> usize {
        self.max_pending
    }
}

impl Drop for OrderedHeaderVerificationWindow {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_payloads::Header;

    #[test]
    fn mismatched_current_block_disables_and_discards_the_suffix() {
        let mut window = OrderedHeaderVerificationWindow::default();
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        drop(result_tx);
        window.pending.push_back(PendingHeaderVerification {
            index: 1,
            hash: neo_primitives::UInt256::from([1u8; 32]),
            ticket: super::super::SignatureVerificationTicket {
                receiver: result_rx,
                metrics: Arc::new(super::super::SignatureVerificationPoolMetrics::default()),
            },
        });
        let mut header = Header::new();
        header.set_index(1);
        let block = Block::from_parts(header, Vec::new());

        assert!(window.take_current(&block).is_none());
        assert!(!window.active);
        assert!(window.pending.is_empty());
        assert!(window.cancellation.is_cancelled());
    }

    #[test]
    fn dropping_window_cancels_queued_suffix() {
        let window = OrderedHeaderVerificationWindow::default();
        let cancellation = Arc::clone(&window.cancellation.cancelled);

        drop(window);

        assert!(cancellation.load(std::sync::atomic::Ordering::Acquire));
    }
}
