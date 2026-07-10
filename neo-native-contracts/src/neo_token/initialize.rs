//! NeoToken genesis initialization.
//!
//! Seeds the exact C# genesis storage records for NEO while keeping the native
//! contract root focused on identity, metadata, hooks, and dispatch delegation.

use super::{DEFAULT_GAS_PER_BLOCK, DEFAULT_REGISTER_PRICE, NEO_TOTAL_AMOUNT, NeoToken};
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::ApplicationEngine;
use neo_storage::StorageItem;
use num_bigint::BigInt;

impl NeoToken {
    /// C# `NeoToken.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (NEO is genesis-active, so this runs while persisting block 0): seed the
    /// committee cache with the standby committee (zero votes each), an empty
    /// voters count, the genesis 5-GAS gas-per-block record at index 0, the
    /// 1000-GAS register price, and mint `TotalAmount` NEO to the BFT address of
    /// the standby validators.
    pub(super) fn initialize_native<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
    ) -> CoreResult<()> {
        let standby_committee = engine.protocol_settings().standby_committee.clone();
        let standby_validators = engine.protocol_settings().standby_validators();
        let snapshot = engine.snapshot_cache();
        let members: Vec<(ECPoint, BigInt)> = standby_committee
            .into_iter()
            .map(|point| (point, BigInt::from(0)))
            .collect();
        snapshot.add(
            Self::committee_key(),
            StorageItem::from_bytes(Self::encode_committee(&members)?),
        );
        // C# `new StorageItem(Array.Empty<byte>())` — BigInteger zero is stored
        // as empty bytes.
        snapshot.add(
            Self::voters_count_key(),
            StorageItem::from_bytes(Vec::new()),
        );
        snapshot.add(
            Self::gas_per_block_key(0),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_GAS_PER_BLOCK,
            ))),
        );
        snapshot.add(
            Self::register_price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_REGISTER_PRICE,
            ))),
        );
        let bft = Self::bft_address(&standby_validators)?;
        self.neo_mint(engine, &bft, &BigInt::from(NEO_TOTAL_AMOUNT), false)
    }
}
