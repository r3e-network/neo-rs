//! # neo-benches::storage_workload
//!
//! Deterministic, streaming MPT persistence workloads.
//!
//! Backend bakeoffs must consume the same operation stream. This module keeps
//! workload construction independent of Criterion and of any storage backend
//! so prefill, commit, and read campaigns can share one measured fixture.
//!
//! ## Boundary
//!
//! This module defines backend-neutral benchmark inputs and deterministic
//! generation. It does not choose a persistence implementation or publish node
//! state.
//!
//! ## Contents
//!
//! - Measured MainNet workload shapes and value-size distributions.
//! - Deterministic prefill and mutation operation generation.
//! - Workload validation, digests, and campaign iteration.

use neo_crypto::Crypto;
use std::fmt::{Display, Formatter};
use std::ops::Range;

/// Prefix byte used by StateService for persisted MPT nodes.
pub const MPT_NODE_PREFIX: u8 = 0xf0;

/// Exact persisted key width: one namespace byte and one 256-bit node hash.
pub const MPT_NODE_KEY_BYTES: usize = 33;

/// Boundaries exported by the durable MDBX value-size instrumentation.
pub const VALUE_SIZE_BUCKET_UPPER_BOUNDS: [Option<usize>; 8] = [
    Some(64),
    Some(128),
    Some(256),
    Some(512),
    Some(1_024),
    Some(4_096),
    Some(16_384),
    None,
];

/// Exact current-state prefill measured before the next persistence campaign.
///
/// The source database was at height 1,857,000 with state root
/// `0x924d0f3d2b93b5c5ecf67c362b5595b68b7023c0f8ed6363d2e501c9bf28cc62`.
/// `neo-db-probe --build-mpt-prefix-index` streamed the coordinated
/// StateService namespace at MDBX transaction 158,387. The 24-bit artifact had
/// SHA-256 `dac4cdff3ad42feafc235a05b900cf377fbfbbfc7662b96792de484dfd2ec562`.
pub const MAINNET_H1_857_000_PREFILL_ROWS: u64 = 115_140_640;

/// Exact StateService MPT put histogram for blocks 1,877,001 through 1,887,000.
/// The replay advanced root
/// `0x455cd1de5682c099da262b3ef351409a90c5f48983cf0b9bc3378eaf1574e981`
/// to `0x4428fcb771363a356985a1618b97cdcf63368e5e24601dc53e9a982a797e2664`;
/// both public reference nodes and two local reopen probes matched the latter.
pub const MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_COUNTS: [u64; 8] =
    [280_486, 241_348, 116_787, 214_297, 155_028, 10, 2, 2];

/// Exact StateService MPT value bytes in each histogram bucket.
pub const MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_BYTES: [u64; 8] = [
    8_998_915, 21_223_494, 23_067_620, 93_157_068, 82_587_579, 20_639, 15_896, 44_268,
];

/// StateService MPT puts in the measured replay window.
pub const MAINNET_H1_877_001_TO_H1_887_000_MPT_PUTS: u64 = 1_007_960;

/// StateService MPT tombstones in the measured replay window.
pub const MAINNET_H1_877_001_TO_H1_887_000_MPT_TOMBSTONES: u64 = 0;

/// Exact StateService MPT logical put bytes in the measured replay window.
pub const MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_BYTES: u64 = 229_115_479;

/// Backing-version lookup hits in the measured replay window.
pub const MAINNET_H1_877_001_TO_H1_887_000_VERSION_HITS: u64 = 8_556;

/// Backing-version lookup misses in the measured replay window.
pub const MAINNET_H1_877_001_TO_H1_887_000_VERSION_MISSES: u64 = 994_869;

/// Deterministic full-scale persistence corpus for blocks 1,877,001 through
/// 1,887,000.
///
/// Every class preserves both the exact measured put count and exact measured
/// byte total. Eight durable MDBX commits covered the window. The observed
/// backing-hit ratio is projected onto the generated operations with
/// nearest-integer rounding, producing 8,595 existing-version operations. The
/// latest exact namespace prefill remains the read-only scan at height
/// 1,857,000; it is intentionally not inferred from mutation counts.
pub const MAINNET_H1_877_001_TO_H1_887_000: WorkloadShape = WorkloadShape {
    source: "neo-n3-mainnet-1877001-1887000",
    seed: 0x4e45_4f33_1877_1887,
    prefill_rows: MAINNET_H1_857_000_PREFILL_ROWS,
    blocks: 10_000,
    commit_count: 8,
    put_count: MAINNET_H1_877_001_TO_H1_887_000_MPT_PUTS,
    tombstone_count: MAINNET_H1_877_001_TO_H1_887_000_MPT_TOMBSTONES,
    version_hit_count: 8_595,
    value_sizes: [
        ValueSizeClass::new(280_486, 8_998_915),
        ValueSizeClass::new(241_348, 21_223_494),
        ValueSizeClass::new(116_787, 23_067_620),
        ValueSizeClass::new(214_297, 93_157_068),
        ValueSizeClass::new(155_028, 82_587_579),
        ValueSizeClass::new(10, 20_639),
        ValueSizeClass::new(2, 15_896),
        ValueSizeClass::new(2, 44_268),
    ],
};

/// One measured put-value size class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueSizeClass {
    /// Number of puts observed in this class during the measured window.
    pub put_count: u64,
    /// Total bytes observed across those puts.
    pub total_bytes: u64,
}

impl ValueSizeClass {
    /// Creates one measured value-size class.
    pub const fn new(put_count: u64, total_bytes: u64) -> Self {
        Self {
            put_count,
            total_bytes,
        }
    }
}

/// Measured parameters that define one backend-neutral persistence campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadShape {
    /// Stable corpus label included in benchmark reports.
    pub source: &'static str,
    /// Reproducible generator seed.
    pub seed: u64,
    /// Existing MPT rows loaded before timed campaign commits begin.
    pub prefill_rows: u64,
    /// Blocks represented by the measured campaign window.
    pub blocks: u64,
    /// Durable publication fences observed in that window.
    pub commit_count: u32,
    /// Put operations observed in that window.
    pub put_count: u64,
    /// Tombstone operations observed in that window.
    pub tombstone_count: u64,
    /// Operations targeting a key present in the prefilled version set.
    /// Tombstones are included in this count.
    pub version_hit_count: u64,
    /// Put distribution in the eight instrumentation buckets.
    pub value_sizes: [ValueSizeClass; 8],
}

/// Why a measured workload cannot produce a valid deterministic campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidWorkloadShape(&'static str);

impl Display for InvalidWorkloadShape {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for InvalidWorkloadShape {}

/// Validated workload plus cached permutation parameters.
#[derive(Debug, Clone)]
pub struct WorkloadCampaign {
    shape: WorkloadShape,
    operation_multiplier: u64,
    operation_offset: u64,
    value_multiplier: u64,
    value_offset: u64,
    hit_offset: u64,
    value_length_floors: [usize; 8],
    value_length_ceil_counts: [u64; 8],
}

impl WorkloadCampaign {
    /// Validates a measured shape and prepares deterministic permutations.
    pub fn new(shape: WorkloadShape) -> Result<Self, InvalidWorkloadShape> {
        validate_shape(&shape)?;
        let operation_count = shape
            .put_count
            .checked_add(shape.tombstone_count)
            .ok_or(InvalidWorkloadShape("operation count overflows u64"))?;
        let mut value_length_floors = [0usize; 8];
        let mut value_length_ceil_counts = [0u64; 8];
        for (index, value_size) in shape.value_sizes.iter().enumerate() {
            if value_size.put_count == 0 {
                continue;
            }
            value_length_floors[index] =
                usize::try_from(value_size.total_bytes / value_size.put_count).map_err(|_| {
                    InvalidWorkloadShape("generated value length does not fit usize")
                })?;
            value_length_ceil_counts[index] = value_size.total_bytes % value_size.put_count;
        }
        Ok(Self {
            operation_multiplier: coprime_multiplier(operation_count, shape.seed ^ 0x6f70),
            operation_offset: bounded_mix(shape.seed ^ 0x6f66, operation_count),
            value_multiplier: coprime_multiplier(shape.put_count, shape.seed ^ 0x7661),
            value_offset: bounded_mix(shape.seed ^ 0x766f, shape.put_count),
            hit_offset: bounded_mix(shape.seed ^ 0x6869, shape.prefill_rows),
            value_length_floors,
            value_length_ceil_counts,
            shape,
        })
    }

    /// Returns the validated measured parameters.
    pub fn shape(&self) -> &WorkloadShape {
        &self.shape
    }

    /// Total operations across every commit in the timed window.
    pub fn operation_count(&self) -> u64 {
        self.shape.put_count + self.shape.tombstone_count
    }

    /// Exact generated logical put bytes across the timed window.
    pub fn logical_value_bytes(&self) -> u128 {
        self.shape
            .value_sizes
            .iter()
            .map(|value_size| u128::from(value_size.total_bytes))
            .sum()
    }

    /// Streams the deterministic prefill without materialising the full state.
    pub fn prefill(&self) -> PrefillOperations<'_> {
        PrefillOperations {
            campaign: self,
            ordinals: 0..self.shape.prefill_rows,
        }
    }

    /// Partitions blocks and operations across the measured commit cadence.
    pub fn commits(&self) -> CommitBatches<'_> {
        CommitBatches {
            campaign: self,
            commit_index: 0,
        }
    }

    fn prefill_at(&self, ordinal: u64) -> WorkloadOperation {
        let put_slot = ordinal % self.shape.put_count;
        let value_slot = permute(
            put_slot,
            self.shape.put_count,
            self.value_multiplier,
            self.value_offset,
        );
        let value_len = self.value_len(value_slot);
        WorkloadOperation {
            key: mpt_key(self.shape.seed, 0x7072_6566_696c_6c00, ordinal),
            kind: OperationKind::Put(deterministic_value(
                self.shape.seed ^ 0x7072_6566,
                ordinal,
                value_len,
            )),
            version_hit: false,
        }
    }

    fn operation_at(&self, ordinal: u64) -> WorkloadOperation {
        let operation_slot = permute(
            ordinal,
            self.operation_count(),
            self.operation_multiplier,
            self.operation_offset,
        );
        let version_hit = operation_slot < self.shape.version_hit_count;
        let key = if version_hit {
            let prefill_ordinal = (operation_slot + self.hit_offset) % self.shape.prefill_rows;
            mpt_key(self.shape.seed, 0x7072_6566_696c_6c00, prefill_ordinal)
        } else {
            mpt_key(self.shape.seed, 0x6e65_772d_6e6f_6465, ordinal)
        };
        let kind = if operation_slot < self.shape.tombstone_count {
            OperationKind::Tombstone
        } else {
            let put_slot = operation_slot - self.shape.tombstone_count;
            let value_slot = permute(
                put_slot,
                self.shape.put_count,
                self.value_multiplier,
                self.value_offset,
            );
            let value_len = self.value_len(value_slot);
            OperationKind::Put(deterministic_value(
                self.shape.seed ^ 0x6361_6d70,
                ordinal,
                value_len,
            ))
        };
        WorkloadOperation {
            key,
            kind,
            version_hit,
        }
    }

    fn value_len(&self, mut value_slot: u64) -> usize {
        for (index, value_size) in self.shape.value_sizes.iter().enumerate() {
            if value_slot < value_size.put_count {
                return self.value_length_floors[index]
                    + usize::from(value_slot < self.value_length_ceil_counts[index]);
            }
            value_slot -= value_size.put_count;
        }
        unreachable!("validated value-size counts cover every put slot")
    }
}

/// One exact raw MPT operation consumed by every persistence backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadOperation {
    /// `0xf0 || 32-byte pseudo-random node hash`.
    pub key: [u8; MPT_NODE_KEY_BYTES],
    /// Exact generated put bytes or a tombstone.
    pub kind: OperationKind,
    /// Whether this operation targets the prefilled version set.
    pub version_hit: bool,
}

/// Put or tombstone operation for one node hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationKind {
    /// A byte-for-byte value to persist.
    Put(Vec<u8>),
    /// A deletion masking older versions of this key.
    Tombstone,
}

/// Streaming iterator over the high-height prefill set.
pub struct PrefillOperations<'a> {
    campaign: &'a WorkloadCampaign,
    ordinals: Range<u64>,
}

impl Iterator for PrefillOperations<'_> {
    type Item = WorkloadOperation;

    fn next(&mut self) -> Option<Self::Item> {
        self.ordinals
            .next()
            .map(|ordinal| self.campaign.prefill_at(ordinal))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        range_size_hint(&self.ordinals)
    }
}

/// One durable commit epoch in the measured cadence.
#[derive(Debug, Clone)]
pub struct CommitBatch<'a> {
    campaign: &'a WorkloadCampaign,
    /// Zero-based commit position in the campaign.
    pub commit_index: u32,
    /// Number of source blocks represented by this commit.
    pub blocks: u64,
    operation_range: Range<u64>,
}

impl CommitBatch<'_> {
    /// Number of raw MPT operations in this commit.
    pub fn operation_count(&self) -> u64 {
        self.operation_range.end - self.operation_range.start
    }

    /// Streams this commit's backend-neutral operation sequence.
    pub fn operations(&self) -> BatchOperations<'_> {
        BatchOperations {
            campaign: self.campaign,
            ordinals: self.operation_range.clone(),
        }
    }
}

/// Iterator over measured durable commit epochs.
pub struct CommitBatches<'a> {
    campaign: &'a WorkloadCampaign,
    commit_index: u32,
}

impl<'a> Iterator for CommitBatches<'a> {
    type Item = CommitBatch<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.commit_index >= self.campaign.shape.commit_count {
            return None;
        }
        let index = self.commit_index;
        self.commit_index += 1;
        let parts = self.campaign.shape.commit_count as u64;
        let operation_start = partition_boundary(self.campaign.operation_count(), parts, index);
        let operation_end = partition_boundary(self.campaign.operation_count(), parts, index + 1);
        let block_start = partition_boundary(self.campaign.shape.blocks, parts, index);
        let block_end = partition_boundary(self.campaign.shape.blocks, parts, index + 1);
        Some(CommitBatch {
            campaign: self.campaign,
            commit_index: index,
            blocks: block_end - block_start,
            operation_range: operation_start..operation_end,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.campaign.shape.commit_count - self.commit_index) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CommitBatches<'_> {}

/// Streaming iterator over one commit's operations.
pub struct BatchOperations<'a> {
    campaign: &'a WorkloadCampaign,
    ordinals: Range<u64>,
}

impl Iterator for BatchOperations<'_> {
    type Item = WorkloadOperation;

    fn next(&mut self) -> Option<Self::Item> {
        self.ordinals
            .next()
            .map(|ordinal| self.campaign.operation_at(ordinal))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        range_size_hint(&self.ordinals)
    }
}

fn validate_shape(shape: &WorkloadShape) -> Result<(), InvalidWorkloadShape> {
    if shape.source.trim().is_empty() {
        return Err(InvalidWorkloadShape("workload source must not be empty"));
    }
    if shape.prefill_rows == 0 {
        return Err(InvalidWorkloadShape(
            "prefill must contain at least one row",
        ));
    }
    if shape.blocks == 0 {
        return Err(InvalidWorkloadShape(
            "campaign must represent at least one block",
        ));
    }
    if shape.commit_count == 0 || u64::from(shape.commit_count) > shape.blocks {
        return Err(InvalidWorkloadShape(
            "commit count must be in 1..=block count",
        ));
    }
    if shape.put_count == 0 {
        return Err(InvalidWorkloadShape(
            "campaign must contain at least one put",
        ));
    }
    let operation_count = shape
        .put_count
        .checked_add(shape.tombstone_count)
        .ok_or(InvalidWorkloadShape("operation count overflows u64"))?;
    if u64::from(shape.commit_count) > operation_count {
        return Err(InvalidWorkloadShape(
            "commit count must not exceed operation count",
        ));
    }
    if shape.version_hit_count < shape.tombstone_count
        || shape.version_hit_count > operation_count
        || shape.version_hit_count > shape.prefill_rows
    {
        return Err(InvalidWorkloadShape(
            "version hits must include every tombstone and fit the operation and prefill counts",
        ));
    }
    let mut bucket_puts = 0u64;
    let mut lower_bound = 0usize;
    for (index, value_size) in shape.value_sizes.iter().enumerate() {
        let upper_bound = VALUE_SIZE_BUCKET_UPPER_BOUNDS[index];
        if value_size.put_count == 0 && value_size.total_bytes != 0 {
            return Err(InvalidWorkloadShape(
                "zero-count value-size classes must contain zero bytes",
            ));
        }
        let put_count = u128::from(value_size.put_count);
        let total_bytes = u128::from(value_size.total_bytes);
        let minimum_bytes = (lower_bound as u128) * put_count;
        let maximum_bytes = upper_bound.map(|upper| (upper as u128) * put_count);
        if total_bytes < minimum_bytes || maximum_bytes.is_some_and(|maximum| total_bytes > maximum)
        {
            return Err(InvalidWorkloadShape(
                "value-size byte total cannot be represented inside its instrumentation bucket",
            ));
        }
        bucket_puts = bucket_puts
            .checked_add(value_size.put_count)
            .ok_or(InvalidWorkloadShape("value-size put count overflows u64"))?;
        lower_bound = upper_bound.map_or(16_385, |upper| upper + 1);
    }
    if bucket_puts != shape.put_count {
        return Err(InvalidWorkloadShape(
            "value-size bucket counts must sum to the measured put count",
        ));
    }
    Ok(())
}

fn partition_boundary(total: u64, parts: u64, boundary: u32) -> u64 {
    ((u128::from(total) * u128::from(boundary)) / u128::from(parts)) as u64
}

fn permute(index: u64, modulus: u64, multiplier: u64, offset: u64) -> u64 {
    ((u128::from(index) * u128::from(multiplier) + u128::from(offset)) % u128::from(modulus)) as u64
}

fn coprime_multiplier(modulus: u64, seed: u64) -> u64 {
    if modulus == 1 {
        return 0;
    }
    let mut candidate = bounded_mix(seed, modulus).max(1);
    while greatest_common_divisor(candidate, modulus) != 1 {
        candidate += 1;
        if candidate == modulus {
            candidate = 1;
        }
    }
    candidate
}

fn greatest_common_divisor(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn bounded_mix(seed: u64, modulus: u64) -> u64 {
    mix64(seed) % modulus
}

fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn mpt_key(seed: u64, domain: u64, ordinal: u64) -> [u8; MPT_NODE_KEY_BYTES] {
    let mut material = [0u8; 24];
    material[..8].copy_from_slice(&seed.to_le_bytes());
    material[8..16].copy_from_slice(&domain.to_le_bytes());
    material[16..].copy_from_slice(&ordinal.to_le_bytes());
    let mut key = [0u8; MPT_NODE_KEY_BYTES];
    key[0] = MPT_NODE_PREFIX;
    key[1..].copy_from_slice(&Crypto::hash256(&material));
    key
}

fn deterministic_value(seed: u64, ordinal: u64, len: usize) -> Vec<u8> {
    let mut value = vec![0u8; len];
    let mut state = mix64(seed ^ ordinal);
    for chunk in value.chunks_mut(u64::BITS as usize / 8) {
        state = mix64(state);
        chunk.copy_from_slice(&state.to_le_bytes()[..chunk.len()]);
    }
    value
}

fn range_size_hint(range: &Range<u64>) -> (usize, Option<usize>) {
    let remaining = range.end - range.start;
    match usize::try_from(remaining) {
        Ok(remaining) => (remaining, Some(remaining)),
        Err(_) => (usize::MAX, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const REPRESENTATIVE_LENGTHS: [usize; 8] = [32, 96, 192, 384, 768, 2_048, 8_192, 32_768];

    fn shape() -> WorkloadShape {
        WorkloadShape {
            source: "unit-test",
            seed: 0x4e45_4f33_4d50_5401,
            prefill_rows: 23,
            blocks: 10,
            commit_count: 3,
            put_count: 10,
            tombstone_count: 2,
            version_hit_count: 4,
            value_sizes: std::array::from_fn(|index| {
                let put_count = [4, 3, 2, 1, 0, 0, 0, 0][index];
                let extra_byte = usize::from(index < 2 && put_count != 0);
                ValueSizeClass::new(
                    put_count,
                    (REPRESENTATIVE_LENGTHS[index] * put_count as usize + extra_byte) as u64,
                )
            }),
        }
    }

    #[test]
    fn campaign_is_deterministic_and_matches_every_measured_count() {
        let campaign = WorkloadCampaign::new(shape()).expect("valid campaign");
        let first = campaign
            .commits()
            .flat_map(|batch| batch.operations().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let second = campaign
            .commits()
            .flat_map(|batch| batch.operations().collect::<Vec<_>>())
            .collect::<Vec<_>>();

        assert_eq!(first, second);
        assert_eq!(first.len(), 12);
        assert_eq!(
            first
                .iter()
                .filter(|operation| matches!(operation.kind, OperationKind::Tombstone))
                .count(),
            2
        );
        assert_eq!(
            first
                .iter()
                .filter(|operation| operation.version_hit)
                .count(),
            4
        );
        let mut lengths = first
            .iter()
            .filter_map(|operation| match &operation.kind {
                OperationKind::Put(value) => Some(value.len()),
                OperationKind::Tombstone => None,
            })
            .collect::<Vec<_>>();
        lengths.sort_unstable();
        assert_eq!(lengths, vec![32, 32, 32, 33, 96, 96, 97, 192, 192, 384]);
        assert_eq!(campaign.logical_value_bytes(), 1_186);
        assert!(first.iter().all(|operation| {
            operation.key.len() == MPT_NODE_KEY_BYTES && operation.key[0] == MPT_NODE_PREFIX
        }));
    }

    #[test]
    fn hits_resolve_to_prefill_and_new_puts_use_a_disjoint_domain() {
        let campaign = WorkloadCampaign::new(shape()).expect("valid campaign");
        let prefill = campaign
            .prefill()
            .map(|operation| operation.key)
            .collect::<HashSet<_>>();
        let operations = campaign
            .commits()
            .flat_map(|batch| batch.operations().collect::<Vec<_>>())
            .collect::<Vec<_>>();

        for operation in operations {
            assert_eq!(prefill.contains(&operation.key), operation.version_hit);
            if matches!(operation.kind, OperationKind::Tombstone) {
                assert!(operation.version_hit);
            }
        }
    }

    #[test]
    fn commit_partition_is_contiguous_and_preserves_cadence_totals() {
        let campaign = WorkloadCampaign::new(shape()).expect("valid campaign");
        let commits = campaign.commits().collect::<Vec<_>>();
        assert_eq!(commits.len(), 3);
        assert_eq!(commits.iter().map(|batch| batch.blocks).sum::<u64>(), 10);
        assert_eq!(
            commits
                .iter()
                .map(CommitBatch::operation_count)
                .sum::<u64>(),
            12
        );
        assert_eq!(commits.iter().flat_map(CommitBatch::operations).count(), 12);
    }

    #[test]
    fn invalid_measured_shapes_fail_before_generating_data() {
        let mut invalid = shape();
        invalid.value_sizes[0].put_count += 1;
        assert_eq!(
            WorkloadCampaign::new(invalid).unwrap_err(),
            InvalidWorkloadShape("value-size bucket counts must sum to the measured put count")
        );

        let mut invalid = shape();
        invalid.version_hit_count = 1;
        assert_eq!(
            WorkloadCampaign::new(invalid).unwrap_err(),
            InvalidWorkloadShape(
                "version hits must include every tombstone and fit the operation and prefill counts"
            )
        );

        let mut invalid = shape();
        invalid.value_sizes[0].total_bytes = 257;
        assert_eq!(
            WorkloadCampaign::new(invalid).unwrap_err(),
            InvalidWorkloadShape(
                "value-size byte total cannot be represented inside its instrumentation bucket"
            )
        );

        let mut invalid = shape();
        invalid.value_sizes[7].total_bytes = 1;
        assert_eq!(
            WorkloadCampaign::new(invalid).unwrap_err(),
            InvalidWorkloadShape("zero-count value-size classes must contain zero bytes")
        );
    }

    #[test]
    fn production_fixture_preserves_the_exact_measured_invariants() {
        assert_eq!(
            MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_COUNTS
                .iter()
                .sum::<u64>(),
            MAINNET_H1_877_001_TO_H1_887_000_MPT_PUTS
        );
        assert_eq!(
            MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_BYTES
                .iter()
                .sum::<u64>(),
            MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_BYTES
        );
        for (index, value_size) in MAINNET_H1_877_001_TO_H1_887_000
            .value_sizes
            .iter()
            .enumerate()
        {
            assert_eq!(
                value_size.put_count,
                MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_COUNTS[index]
            );
            assert_eq!(
                value_size.total_bytes,
                MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_BYTES[index]
            );
        }
        let observations = MAINNET_H1_877_001_TO_H1_887_000_VERSION_HITS
            + MAINNET_H1_877_001_TO_H1_887_000_VERSION_MISSES;
        let projected_hits = ((u128::from(MAINNET_H1_877_001_TO_H1_887_000_MPT_PUTS)
            * u128::from(MAINNET_H1_877_001_TO_H1_887_000_VERSION_HITS))
            + u128::from(observations / 2))
            / u128::from(observations);

        let campaign = WorkloadCampaign::new(MAINNET_H1_877_001_TO_H1_887_000)
            .expect("measured production campaign must validate");
        assert_eq!(campaign.operation_count(), 1_007_960);
        assert_eq!(campaign.shape().version_hit_count, projected_hits as u64);
        assert_eq!(
            campaign.logical_value_bytes(),
            u128::from(MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_BYTES)
        );
        assert_eq!(campaign.commits().len(), 8);
        assert_eq!(
            campaign.commits().map(|commit| commit.blocks).sum::<u64>(),
            10_000
        );
        assert_eq!(
            campaign
                .commits()
                .map(|commit| commit.operation_count())
                .sum::<u64>(),
            1_007_960
        );

        let mut generated_counts = [0u64; 8];
        let mut generated_bytes = [0u64; 8];
        for value_slot in 0..campaign.shape().put_count {
            let value_len = campaign.value_len(value_slot);
            let bucket = VALUE_SIZE_BUCKET_UPPER_BOUNDS
                .iter()
                .position(|upper| upper.is_none_or(|upper| value_len <= upper))
                .expect("the final value-size bucket is unbounded");
            generated_counts[bucket] += 1;
            generated_bytes[bucket] += value_len as u64;
        }
        assert_eq!(
            generated_counts,
            MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_COUNTS
        );
        assert_eq!(
            generated_bytes,
            MAINNET_H1_877_001_TO_H1_887_000_MPT_VALUE_SIZE_BYTES
        );
    }
}
