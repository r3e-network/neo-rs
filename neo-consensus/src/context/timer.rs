//! dBFT round timing for [`ConsensusContext`]: the view-change backoff, the
//! primary `PrepareRequest` delays, recovery resends, and the C#
//! `ExtendTimerByFactor` deadline extension.

use super::{ConsensusContext, DEFAULT_BLOCK_TIME_MS};

impl ConsensusContext {
    /// Gets the timeout duration for the current view
    #[must_use]
    pub fn get_timeout(&self) -> u64 {
        // Base timeout + exponential backoff for view changes, matching C#
        // `TimePerBlock << (ViewNumber + 1)`. The exponent is capped at 5 (a
        // deliberate, safe deviation): C# performs a 32-bit `int` shift that
        // overflows into garbage from ~view 17 and wraps from view 31, whereas an
        // unbounded `u64` shift here would panic. The cap only differs from C# at
        // view >= 5 — a severely degraded consensus that does not occur in
        // practice, and where C#'s own value is already nonsensical — so it does
        // not affect block production or state, only liveness timing in an
        // unreachable regime.
        self.base_block_time() << (self.view_number + 1).min(5)
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
        // Exponent capped at 5 for the same safety reason as `get_timeout`: an
        // unbounded `u64` shift would panic, and the cap only diverges from C#'s
        // overflow-prone 32-bit int shift at the unreachable view >= 4 regime.
        self.base_block_time() << (expected_view + 1).min(5)
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
