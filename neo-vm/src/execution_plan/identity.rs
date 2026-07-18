//! Exact, versioned identities for immutable execution plans.

use neo_crypto::Crypto;
use neo_primitives::{Hardfork, TriggerType, UInt160};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Schema version for an [`ExecutionPlanKey`].
///
/// A new version is required when plan construction gains a semantic input
/// that is not represented by the current key.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
pub struct ExecutionPlanKeyVersion(u16);

impl ExecutionPlanKeyVersion {
    /// Initial exact-byte, protocol, hardfork, trigger, and contract identity.
    pub const V1: Self = Self(1);

    /// Returns the numeric key schema version.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Neo protocol release whose execution semantics produced a plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ProtocolVersion {
    /// Neo N3 v3.10.1, the current compatibility target.
    pub const NEO_N3_V3_10_1: Self = Self::new(3, 10, 1);

    /// Creates a semantic protocol release identity.
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns the major release component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the minor release component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Returns the patch release component.
    #[must_use]
    pub const fn patch(self) -> u16 {
        self.patch
    }
}

/// Network and semantic protocol release used to resolve a plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProtocolIdentity {
    network_magic: u32,
    version: ProtocolVersion,
}

impl ProtocolIdentity {
    /// Creates a network-scoped semantic protocol identity.
    #[must_use]
    pub const fn new(network_magic: u32, version: ProtocolVersion) -> Self {
        Self {
            network_magic,
            version,
        }
    }

    /// Returns the Neo network magic.
    #[must_use]
    pub const fn network_magic(self) -> u32 {
        self.network_magic
    }

    /// Returns the semantic protocol release.
    #[must_use]
    pub const fn version(self) -> ProtocolVersion {
        self.version
    }
}

/// One hardfork's configured schedule and applicability to an execution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum HardforkPlanState {
    /// The hardfork is absent from the protocol schedule.
    Unconfigured,
    /// The hardfork is configured but does not apply to this execution.
    Pending {
        /// Configured activation height.
        activation_height: u32,
    },
    /// The hardfork is configured and applies to this execution.
    Active {
        /// Configured activation height.
        activation_height: u32,
    },
}

impl HardforkPlanState {
    /// Returns the configured activation height, if any.
    #[must_use]
    pub const fn activation_height(self) -> Option<u32> {
        match self {
            Self::Unconfigured => None,
            Self::Pending { activation_height } | Self::Active { activation_height } => {
                Some(activation_height)
            }
        }
    }

    /// Returns whether this hardfork applies to the execution.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active { .. })
    }
}

/// Complete known hardfork table and the state applicable to an execution.
///
/// The fixed array is indexed by [`Hardfork::index`]. It records absent forks,
/// configured activation heights, and whether each configured fork applies.
/// This also covers verification engines where configured forks may apply
/// without a persisting-block height.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HardforkTableIdentity {
    states: [HardforkPlanState; Hardfork::COUNT],
}

impl HardforkTableIdentity {
    /// Creates a table with every known hardfork explicitly unconfigured.
    #[must_use]
    pub const fn unconfigured() -> Self {
        Self {
            states: [HardforkPlanState::Unconfigured; Hardfork::COUNT],
        }
    }

    /// Creates a table from states in [`Hardfork::ALL`] order.
    #[must_use]
    pub const fn new(states: [HardforkPlanState; Hardfork::COUNT]) -> Self {
        Self { states }
    }

    /// Returns a new table with one hardfork state replaced.
    #[must_use]
    pub const fn with_state(mut self, hardfork: Hardfork, state: HardforkPlanState) -> Self {
        self.states[hardfork.index() as usize] = state;
        self
    }

    /// Returns the state for one hardfork.
    #[must_use]
    pub const fn state(self, hardfork: Hardfork) -> HardforkPlanState {
        self.states[hardfork.index() as usize]
    }

    /// Returns all states in [`Hardfork::ALL`] order.
    #[must_use]
    pub const fn states(&self) -> &[HardforkPlanState; Hardfork::COUNT] {
        &self.states
    }
}

impl Default for HardforkTableIdentity {
    fn default() -> Self {
        Self::unconfigured()
    }
}

/// Exact deployed-contract version used while resolving a script context.
///
/// Contract hash alone is insufficient because it survives contract updates.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContractResolutionIdentity {
    contract_hash: UInt160,
    contract_id: i32,
    update_counter: u16,
    nef_checksum: u32,
}

impl ContractResolutionIdentity {
    /// Creates an exact deployed-contract resolution identity.
    #[must_use]
    pub const fn new(
        contract_hash: UInt160,
        contract_id: i32,
        update_counter: u16,
        nef_checksum: u32,
    ) -> Self {
        Self {
            contract_hash,
            contract_id,
            update_counter,
            nef_checksum,
        }
    }

    /// Returns the logical contract hash.
    #[must_use]
    pub const fn contract_hash(self) -> UInt160 {
        self.contract_hash
    }

    /// Returns the persisted contract ID.
    #[must_use]
    pub const fn contract_id(self) -> i32 {
        self.contract_id
    }

    /// Returns the persisted contract update counter.
    #[must_use]
    pub const fn update_counter(self) -> u16 {
        self.update_counter
    }

    /// Returns the checksum of the resolved NEF.
    #[must_use]
    pub const fn nef_checksum(self) -> u32 {
        self.nef_checksum
    }
}

/// Immutable identity for a verified execution plan.
///
/// This key identifies decoded or otherwise pre-verified execution structure;
/// it never identifies or stores an execution result. Exact bytes are retained
/// independently of `Script` object identity. Consequently, equal bytecode in
/// two logical contract versions remains separate through
/// [`ContractResolutionIdentity`], while raw scripts can safely share a plan.
#[derive(Clone, Debug)]
pub struct ExecutionPlanKey {
    version: ExecutionPlanKeyVersion,
    script_hash: [u8; 20],
    script_bytes: Arc<[u8]>,
    entry_ip: u32,
    protocol: ProtocolIdentity,
    hardforks: HardforkTableIdentity,
    trigger_bits: u8,
    contract: Option<ContractResolutionIdentity>,
}

impl ExecutionPlanKey {
    /// Creates a v1 plan identity from exact immutable bytecode.
    #[must_use]
    pub fn new(
        script_bytes: Arc<[u8]>,
        entry_ip: u32,
        protocol: ProtocolIdentity,
        hardforks: HardforkTableIdentity,
        trigger: TriggerType,
        contract: Option<ContractResolutionIdentity>,
    ) -> Self {
        let script_hash = Crypto::hash160(script_bytes.as_ref());
        Self {
            version: ExecutionPlanKeyVersion::V1,
            script_hash,
            script_bytes,
            entry_ip,
            protocol,
            hardforks,
            trigger_bits: trigger.bits(),
            contract,
        }
    }

    /// Returns the key schema version.
    #[must_use]
    pub const fn version(&self) -> ExecutionPlanKeyVersion {
        self.version
    }

    /// Returns Neo's protocol Hash160 for the retained bytecode.
    #[must_use]
    pub const fn script_hash(&self) -> &[u8; 20] {
        &self.script_hash
    }

    /// Returns the exact retained bytecode.
    #[must_use]
    pub fn script_bytes(&self) -> &[u8] {
        self.script_bytes.as_ref()
    }

    /// Returns the retained byte count for cache accounting.
    #[must_use]
    pub fn script_len(&self) -> usize {
        self.script_bytes.len()
    }

    /// Verifies both a candidate protocol hash and its exact byte identity.
    ///
    /// A matching hash is only a bucket-selection hint. Callers must use this
    /// check before accepting a cache hit, so a Hash160 collision cannot reuse
    /// a plan for different bytes.
    #[must_use]
    pub fn matches_script(&self, script_hash: &[u8; 20], script_bytes: &[u8]) -> bool {
        self.script_hash == *script_hash && self.script_bytes.as_ref() == script_bytes
    }

    /// Returns the entry instruction pointer as a byte offset.
    #[must_use]
    pub const fn entry_ip(&self) -> u32 {
        self.entry_ip
    }

    /// Returns the network and protocol release identity.
    #[must_use]
    pub const fn protocol(&self) -> ProtocolIdentity {
        self.protocol
    }

    /// Returns the complete applicable hardfork table identity.
    #[must_use]
    pub const fn hardforks(&self) -> HardforkTableIdentity {
        self.hardforks
    }

    /// Returns the execution trigger.
    #[must_use]
    pub const fn trigger(&self) -> TriggerType {
        TriggerType::from_bits_retain(self.trigger_bits)
    }

    /// Returns exact deployed-contract resolution identity, when applicable.
    #[must_use]
    pub const fn contract(&self) -> Option<ContractResolutionIdentity> {
        self.contract
    }
}

impl PartialEq for ExecutionPlanKey {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
            && self.script_hash == other.script_hash
            && self.script_bytes.as_ref() == other.script_bytes.as_ref()
            && self.entry_ip == other.entry_ip
            && self.protocol == other.protocol
            && self.hardforks == other.hardforks
            && self.trigger_bits == other.trigger_bits
            && self.contract == other.contract
    }
}

impl Eq for ExecutionPlanKey {}

impl Hash for ExecutionPlanKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.version.hash(state);
        self.script_hash.hash(state);
        self.script_bytes.len().hash(state);
        self.entry_ip.hash(state);
        self.protocol.hash(state);
        self.hardforks.hash(state);
        self.trigger_bits.hash(state);
        self.contract.hash(state);
    }
}

#[cfg(test)]
#[path = "../tests/execution_plan/identity.rs"]
mod tests;
