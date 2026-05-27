//! Bloom filter state and filtered-block helpers for `RemoteNode`.

use super::RemoteNode;
use crate::cryptography::BloomFilter;
use crate::network::p2p::payloads::{
    block::Block, filter_add_payload::FilterAddPayload, filter_load_payload::FilterLoadPayload,
    transaction::Transaction,
};
use governor::{
    DefaultDirectRateLimiter, Quota, RateLimiter, middleware::StateInformationMiddleware,
};
use std::net::SocketAddr;
use std::num::NonZeroU32;
use tracing::{debug, warn};

pub(super) struct BloomFilterState {
    filter: Option<BloomFilter>,
    filter_limiter: DefaultDirectRateLimiter<StateInformationMiddleware>,
}

impl Default for BloomFilterState {
    fn default() -> Self {
        Self {
            filter: None,
            filter_limiter: RateLimiter::direct(Self::filter_quota())
                .with_middleware::<StateInformationMiddleware>(),
        }
    }
}

impl BloomFilterState {
    /// Maximum bloom filter size in bytes (matches C# MAX_FILTER_SIZE = 36000).
    const MAX_BLOOM_FILTER_SIZE: usize = 36000;
    /// Maximum number of hash functions for bloom filter (reasonable upper bound).
    const MAX_BLOOM_K: u8 = 50;
    /// Maximum bloom filter operations per minute to prevent DoS attacks.
    const MAX_FILTER_OPS_PER_MINUTE: u32 = 100;

    #[cfg(test)]
    fn filter(&self) -> Option<&BloomFilter> {
        self.filter.as_ref()
    }

    fn filter_quota() -> Quota {
        let max_ops = NonZeroU32::new(Self::MAX_FILTER_OPS_PER_MINUTE)
            .expect("filter operation quota is non-zero");
        Quota::per_minute(max_ops).allow_burst(max_ops)
    }

    pub(super) fn clear(&mut self) {
        self.filter = None;
    }

    pub(super) fn on_filter_load(&mut self, payload: &FilterLoadPayload, endpoint: SocketAddr) {
        if !self.check_filter_rate_limit(endpoint) {
            return;
        }

        if payload.filter.is_empty() || payload.k == 0 {
            self.filter = None;
            return;
        }

        if payload.filter.len() > Self::MAX_BLOOM_FILTER_SIZE {
            warn!(
                target: "neo",
                endpoint = %endpoint,
                filter_size = payload.filter.len(),
                max_size = Self::MAX_BLOOM_FILTER_SIZE,
                "bloom filter too large, rejecting"
            );
            self.filter = None;
            return;
        }

        if payload.k > Self::MAX_BLOOM_K {
            warn!(
                target: "neo",
                endpoint = %endpoint,
                k = payload.k,
                max_k = Self::MAX_BLOOM_K,
                "bloom filter k value too large, rejecting"
            );
            self.filter = None;
            return;
        }

        let bit_size = payload.filter.len() * 8;
        match BloomFilter::with_bits(bit_size, payload.k as usize, payload.tweak, &payload.filter) {
            Ok(filter) => self.filter = Some(filter),
            Err(error) => {
                debug!(target: "neo", %error, "failed to load bloom filter from payload");
                self.filter = None;
            }
        }
    }

    pub(super) fn on_filter_add(&mut self, payload: &FilterAddPayload, endpoint: SocketAddr) {
        if !self.check_filter_rate_limit(endpoint) {
            return;
        }

        if let Some(filter) = self.filter.as_mut() {
            filter.add(&payload.data);
        }
    }

    fn check_filter_rate_limit(&mut self, endpoint: SocketAddr) -> bool {
        match self.filter_limiter.check() {
            Ok(_) => true,
            Err(_) => {
                warn!(
                    target: "neo",
                    endpoint = %endpoint,
                    "bloom filter rate limit exceeded, rejecting operation"
                );
                false
            }
        }
    }

    fn flags(&self, block: &Block) -> Option<Vec<bool>> {
        let filter = self.filter.as_ref()?;
        Some(
            block
                .transactions
                .iter()
                .map(|tx| Self::matches_transaction(filter, tx))
                .collect(),
        )
    }

    fn matches_transaction(filter: &BloomFilter, tx: &Transaction) -> bool {
        let hash = match tx.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed while evaluating bloom filter"
                );
                return false;
            }
        };
        let hash_bytes = hash.to_array();
        if filter.check(&hash_bytes) {
            return true;
        }

        tx.signers().iter().any(|signer| {
            let account_bytes = signer.account.as_bytes();
            filter.check(account_bytes.as_ref())
        })
    }
}

impl RemoteNode {
    pub(super) fn on_filter_load(&mut self, payload: &FilterLoadPayload) {
        self.bloom_filter.on_filter_load(payload, self.endpoint);
    }

    pub(super) fn on_filter_clear(&mut self) {
        self.bloom_filter.clear();
    }

    pub(super) fn on_filter_add(&mut self, payload: &FilterAddPayload) {
        self.bloom_filter.on_filter_add(payload, self.endpoint);
    }

    pub(super) fn bloom_filter_flags(&self, block: &Block) -> Option<Vec<bool>> {
        self.bloom_filter.flags(block)
    }
}

#[cfg(test)]
mod tests {
    use super::BloomFilterState;
    use crate::cryptography::BloomFilter;
    use crate::network::p2p::payloads::{
        block::Block, filter_add_payload::FilterAddPayload, filter_load_payload::FilterLoadPayload,
        signer::Signer, transaction::Transaction, witness::Witness,
    };
    use crate::{UInt160, WitnessScope};
    use neo_vm_rs::OpCode;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn endpoint() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20333)
    }

    fn payload_matching(data: &[u8]) -> FilterLoadPayload {
        let mut filter = BloomFilter::new(256, 3, 0xCAFE_BABE).expect("filter");
        filter.add(data);
        FilterLoadPayload::create_from_bloom_filter(&filter)
    }

    fn transaction_with_script(script: Vec<u8>) -> Transaction {
        transaction_with_account(script, UInt160::zero())
    }

    fn transaction_with_account(script: Vec<u8>, account: UInt160) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(1);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(script);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn filter_load_accepts_valid_filter_payload() {
        let data = b"contract-hash";
        let mut state = BloomFilterState::default();

        state.on_filter_load(&payload_matching(data), endpoint());

        assert!(state.filter().expect("loaded filter").check(data));
    }

    #[test]
    fn filter_load_empty_or_zero_hash_functions_clears_filter() {
        let mut state = BloomFilterState::default();
        state.on_filter_load(&payload_matching(b"existing"), endpoint());

        state.on_filter_load(&FilterLoadPayload::new(Vec::new(), 3, 0), endpoint());
        assert!(state.filter().is_none());

        state.on_filter_load(&payload_matching(b"existing"), endpoint());
        state.on_filter_load(&FilterLoadPayload::new(vec![0xFF], 0, 0), endpoint());
        assert!(state.filter().is_none());
    }

    #[test]
    fn filter_load_rejects_oversized_or_excessive_hash_function_payloads() {
        let mut state = BloomFilterState::default();
        state.on_filter_load(&payload_matching(b"existing"), endpoint());

        state.on_filter_load(
            &FilterLoadPayload::new(vec![0; BloomFilterState::MAX_BLOOM_FILTER_SIZE + 1], 3, 0),
            endpoint(),
        );
        assert!(state.filter().is_none());

        state.on_filter_load(&payload_matching(b"existing"), endpoint());
        state.on_filter_load(
            &FilterLoadPayload::new(vec![0xFF], BloomFilterState::MAX_BLOOM_K + 1, 0),
            endpoint(),
        );
        assert!(state.filter().is_none());
    }

    #[test]
    fn filter_add_updates_loaded_filter() {
        let data = b"new-element";
        let mut state = BloomFilterState::default();
        state.on_filter_load(&FilterLoadPayload::new(vec![0; 32], 3, 0), endpoint());

        state.on_filter_add(&FilterAddPayload::new(data.to_vec()), endpoint());

        assert!(state.filter().expect("loaded filter").check(data));
    }

    #[test]
    fn filter_add_without_loaded_filter_is_noop() {
        let mut state = BloomFilterState::default();

        state.on_filter_add(&FilterAddPayload::new(b"new-element".to_vec()), endpoint());

        assert!(state.filter().is_none());
    }

    #[test]
    fn filter_clear_removes_loaded_filter() {
        let mut state = BloomFilterState::default();
        state.on_filter_load(&payload_matching(b"existing"), endpoint());

        state.clear();

        assert!(state.filter().is_none());
    }

    #[test]
    fn filter_rate_limit_allows_initial_burst_then_rejects() {
        let mut state = BloomFilterState::default();

        for _ in 0..BloomFilterState::MAX_FILTER_OPS_PER_MINUTE {
            assert!(state.check_filter_rate_limit(endpoint()));
        }
        assert!(!state.check_filter_rate_limit(endpoint()));
    }

    #[test]
    fn bloom_filter_rejects_unserializable_transaction_hash() {
        let mut filter = BloomFilter::new(256, 3, 0).expect("filter");
        filter.add(UInt160::zero().as_bytes().as_ref());
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

        assert!(!BloomFilterState::matches_transaction(&filter, &tx));
    }

    #[test]
    fn bloom_filter_matches_valid_transaction_hash() {
        let tx = transaction_with_script(vec![OpCode::PUSH1.byte()]);
        let mut filter = BloomFilter::new(256, 3, 0).expect("filter");
        filter.add(&tx.try_hash().expect("hash").to_array());

        assert!(BloomFilterState::matches_transaction(&filter, &tx));
    }

    #[test]
    fn bloom_filter_matches_transaction_signer_account() {
        let account = UInt160::from([3u8; 20]);
        let tx = transaction_with_account(vec![OpCode::PUSH1.byte()], account);
        let mut filter = BloomFilter::new(256, 3, 0).expect("filter");
        filter.add(account.as_bytes().as_ref());

        assert!(BloomFilterState::matches_transaction(&filter, &tx));
    }

    #[test]
    fn bloom_filter_does_not_match_unrelated_transaction() {
        let tx = transaction_with_script(vec![OpCode::PUSH1.byte()]);
        let mut filter = BloomFilter::new(256, 3, 0).expect("filter");
        filter.add(b"unrelated");

        assert!(!BloomFilterState::matches_transaction(&filter, &tx));
    }

    #[test]
    fn bloom_filter_flags_return_none_without_loaded_filter() {
        let mut block = Block::new();
        block
            .transactions
            .push(transaction_with_script(vec![OpCode::PUSH1.byte()]));
        let state = BloomFilterState::default();

        assert!(state.flags(&block).is_none());
    }

    #[test]
    fn bloom_filter_flags_preserve_transaction_order() {
        let tx1 = transaction_with_script(vec![OpCode::PUSH1.byte()]);
        let tx2 = transaction_with_script(vec![OpCode::PUSH2.byte()]);
        let mut filter = BloomFilter::new(256, 3, 0).expect("filter");
        filter.add(&tx2.try_hash().expect("hash").to_array());
        let mut state = BloomFilterState::default();
        state.on_filter_load(
            &FilterLoadPayload::create_from_bloom_filter(&filter),
            endpoint(),
        );
        let mut block = Block::new();
        block.transactions = vec![tx1, tx2];

        assert_eq!(state.flags(&block), Some(vec![false, true]));
    }
}
