//! Prusti pilot batch 5 — Phase B: dBFT quorum math (neo-consensus context).
//!
//! Mirrors `ConsensusContext::f` / `::m` (neo-consensus/src/context/mod.rs):
//!   f = (n-1)/3, M = n - f, thresholds `count >= M`.
//! Properties proven here are the numeric core of dBFT safety:
//!   - 3f <= n-1 (definition of the floor division),
//!   - M >= 1 and M >= f+1 (a quorum always exceeds the fault bound),
//!   - QUORUM INTERSECTION: 2M - n > f, i.e. any two M-sized subsets of the
//!     n validators share more than f validators, so with at most f faulty
//!     nodes every two quorums overlap in at least one honest node.
//!   - monotonicity of f and M in n,
//!   - `count >= M` implies `count > f` (more_than_f_nodes_committed_or_lost).
//!
//! Bitflag scopes (WitnessScope) live in the Verus counterpart
//! pilot_verus4.rs: Prusti 0.2.2 cannot encode bitwise ops on integers
//! (experimental `encode_bitvectors` ICEs) — see report §4.4.

use prusti_contracts::*;

/// Mirror of `ConsensusContext::f`: faulty nodes tolerated, f = (n-1)/3.
#[pure]
#[requires(1 <= n)]
#[ensures(result == (n - 1) / 3)]
#[ensures(3 * result <= n - 1)]
pub fn f_count(n: usize) -> usize {
    (n - 1) / 3
}

/// Mirror of `ConsensusContext::m`: quorum size, M = n - f.
#[pure]
#[requires(1 <= n)]
#[ensures(result == n - f_count(n))]
#[ensures(result >= 1)]
pub fn m_count(n: usize) -> usize {
    n - f_count(n)
}

// ---- C# parity spots (n=4: f=1 M=3; n=7: f=2 M=5; n=21: f=6 M=15) -----------

#[pure]
#[ensures(f_count(1) == 0)]
pub fn spot_f_1() -> usize {
    f_count(1)
}

#[pure]
#[ensures(f_count(4) == 1)]
pub fn spot_f_4() -> usize {
    f_count(4)
}

#[pure]
#[ensures(f_count(7) == 2)]
pub fn spot_f_7() -> usize {
    f_count(7)
}

#[pure]
#[ensures(f_count(21) == 6)]
pub fn spot_f_21() -> usize {
    f_count(21)
}

#[pure]
#[ensures(m_count(4) == 3)]
pub fn spot_m_4() -> usize {
    m_count(4)
}

#[pure]
#[ensures(m_count(7) == 5)]
pub fn spot_m_7() -> usize {
    m_count(7)
}

#[pure]
#[ensures(m_count(21) == 15)]
pub fn spot_m_21() -> usize {
    m_count(21)
}

// ---- Safety theorems ---------------------------------------------------------

/// A quorum always strictly exceeds the number of tolerated faulty nodes:
/// M >= f+1. This is what makes `count >= M` imply `count > f`.
#[pure]
#[requires(1 <= n)]
#[ensures(m_count(n) >= f_count(n) + 1)]
pub fn m_exceeds_faults(n: usize) -> bool {
    true
}

/// QUORUM INTERSECTION: any two M-sized subsets of n validators share more
/// than f validators (2M - n > f), so with at most f faulty nodes two quorums
/// always overlap in at least one honest node. Guard keeps 2*M in range.
#[pure]
#[requires(1 <= n)]
#[requires(n <= 9223372036854775807)]
#[ensures(2 * m_count(n) - n > f_count(n))]
pub fn quorum_intersection(n: usize) -> bool {
    true
}

/// f is monotone non-decreasing in the validator count.
#[pure]
#[requires(1 <= a)]
#[requires(a <= b)]
#[ensures(f_count(a) <= f_count(b))]
pub fn f_monotonic(a: usize, b: usize) -> bool {
    true
}

/// M is monotone non-decreasing in the validator count.
#[pure]
#[requires(1 <= a)]
#[requires(a <= b)]
#[ensures(m_count(a) <= m_count(b))]
pub fn m_monotonic(a: usize, b: usize) -> bool {
    true
}

// ---- Threshold predicates ----------------------------------------------------

/// Mirror of the `count >= M` checks (`has_enough_commits`,
/// `has_enough_change_views`, `has_enough_prepare_responses` tail).
#[pure]
#[requires(1 <= n)]
#[ensures(result == (count >= m_count(n)))]
pub fn has_enough(count: usize, n: usize) -> bool {
    count >= m_count(n)
}

/// Reaching the M threshold implies strictly more than f participants —
/// the numeric content of `more_than_f_nodes_committed_or_lost`.
#[pure]
#[requires(1 <= n)]
#[requires(has_enough(count, n))]
#[ensures(count > f_count(n))]
pub fn enough_implies_more_than_f(count: usize, n: usize) -> bool {
    true
}

/// Mirror of `more_than_f_nodes_committed_or_lost`: (committed + failed) > f.
#[pure]
#[requires(1 <= n)]
#[requires(committed + failed <= 18446744073709551615)]
#[ensures(result == (committed + failed > f_count(n)))]
pub fn more_than_f_committed_or_lost(committed: usize, failed: usize, n: usize) -> bool {
    committed + failed > f_count(n)
}

/// If a quorum of commits is present, then certainly more than f nodes
/// committed (the two code paths agree on this implication).
#[pure]
#[requires(1 <= n)]
#[requires(has_enough(committed, n))]
#[ensures(more_than_f_committed_or_lost(committed, 0, n))]
pub fn quorum_implies_more_than_f(committed: usize, n: usize) -> bool {
    true
}

fn main() {}
