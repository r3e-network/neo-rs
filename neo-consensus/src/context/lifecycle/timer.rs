//! dBFT round timing for [`ConsensusContext`]: the view-change backoff, the
//! primary `PrepareRequest` delays, recovery resends, and the C#
//! `ExtendTimerByFactor` deadline extension.

use super::{ConsensusContext, DEFAULT_BLOCK_TIME_MS};

impl ConsensusContext {
    /// Gets the timeout duration for the current view
    #[must_use]
    pub fn get_timeout(&self) -> u64 {
        // Base timeout + exponential backoff for view changes, matching C#
        // `TimePerBlock << (ViewNumber + 1)`.
        //
        // C# uses a 32-bit int shift, where the shift amount is masked to 5 bits
        // (i.e., `% 32`). This means the timeout grows exponentially for views 0-30,
        // then wraps around at view 31 (1 << 32 == 1 << 0 for 32-bit int).
        //
        // We replicate this behavior exactly by masking the shift amount to 5 bits.
        let shift = (self.view_number + 1) & 0x1F; // Mask to 5 bits, matching C# behavior
        self.base_block_time() << shift
    }

    /// Non-recovering primary delay before sending a `PrepareRequest`.
    ///
    /// Neo's C# DBFT primary schedules `SendPrepareRequest` after `TimePerBlock`
    /// for normal rounds, including higher views. Recovery paths can extend this
    /// before re-entering the timer loop.
    #[must_use]
    pub fn prepare_request_delay(&self) -> u64 {
        self.base_block_time()
    }

    /// Primary delay after a `PrepareRequest` before requesting a view change.
    ///
    /// Matches C# `SendPrepareRequest`, which schedules
    /// `(TimePerBlock << (ViewNumber + 1)) - TimePerBlock` for view 0, and the
    /// full shifted delay for higher views.
    #[must_use]
    pub fn prepare_request_follow_up_delay(&self) -> u64 {
        let timeout = self.get_timeout();
        if self.view_number == 0 {
            timeout.saturating_sub(self.base_block_time())
        } else {
            timeout
        }
    }

    /// Total primary delay from view start to the post-prepare timeout.
    #[must_use]
    pub fn primary_timeout_delay(&self) -> u64 {
        self.prepare_request_delay()
            .saturating_add(self.prepare_request_follow_up_delay())
    }

    /// Delay between periodic recovery-message resends after local commit.
    #[must_use]
    pub fn commit_recovery_resend_delay(&self) -> u64 {
        self.base_block_time() << 1
    }

    /// Delay before retrying a ChangeView request for the expected next view.
    #[must_use]
    pub fn change_view_retry_delay(&self) -> u64 {
        let expected_view = self.view_number.saturating_add(1);
        // Match C# behavior: 32-bit shift with 5-bit mask (see `get_timeout` for details)
        let shift = (expected_view + 1) & 0x1F;
        self.base_block_time() << shift
    }

    fn base_block_time(&self) -> u64 {
        if self.expected_block_time > 0 {
            self.expected_block_time
        } else {
            DEFAULT_BLOCK_TIME_MS
        }
    }

    /// Checks if the current view has timed out
    #[must_use]
    pub fn is_timed_out(&self, current_time: u64) -> bool {
        current_time >= self.view_start_time + self.get_timeout() + self.timer_extension
    }

    /// Whether this node has already broadcast its Commit for the current view.
    #[must_use]
    pub fn commit_sent(&self) -> bool {
        match self.my_index {
            Some(index) => {
                self.commits.contains_key(&index)
                    && self.commit_view_numbers.get(&index) == Some(&self.view_number)
            }
            None => false,
        }
    }

    /// C# `ExtendTimerByFactor`: push the current view's change-view deadline
    /// later by `max_delay_in_block_times * TimePerBlock / M` (never earlier), so
    /// a round that is making progress (prepare/response/commit received) is not
    /// abandoned to a view change prematurely. No-op for watch-only nodes, while
    /// view-changing, or after this node has sent its commit.
    pub fn extend_timer_by_factor(&mut self, max_delay_in_block_times: u64) {
        if self.my_index.is_none() || self.view_changing() || self.commit_sent() {
            return;
        }
        let m = self.m().max(1) as u64;
        let extension = max_delay_in_block_times.saturating_mul(self.base_block_time()) / m;
        self.timer_extension = self.timer_extension.max(extension);
    }
}
