//! # neo-execution::host_access_audit
//!
//! Deny-by-default host-access declarations for guarded specializations.
//!
//! A specialization receives an immutable policy and may interact with an
//! application engine only through `AuditedApplicationHost`. The audit keeps
//! no unbounded trace: it latches the first undeclared attempt and rejects all
//! later access. Candidate overlays must be discarded whenever `finish` fails.
//!
//! ## Boundary
//!
//! This module defines and enforces bounded access policy. It does not execute
//! scripts, mutate canonical state, or decide whether a candidate is promoted.
//!
//! ## Contents
//!
//! - Exact storage, context, and contract-call access declarations.
//! - Immutable policy construction with bounded limits.
//! - Fail-closed auditing and violation records.

use neo_manifest::CallFlags;
use neo_primitives::{FindOptions, Hardfork, UInt160};
use neo_storage::StorageKey;
use neo_vm::{ContractResolutionIdentity, NativeCacheDomain, RangeDirection};
use std::mem::size_of;
use std::sync::Arc;

mod limits;
pub use limits::{HostAccessPolicyError, HostAccessPolicyLimits};

/// Exact resolved storage range domain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedStorageRangeDomain {
    /// Every storage key across all contract IDs.
    WholeStore,
    /// Every key beginning with one exact prefix.
    Prefix(Vec<u8>),
    /// Lexicographic half-open interval `[start, end)`.
    HalfOpen {
        /// Inclusive start key suffix.
        start: Vec<u8>,
        /// Exclusive end key suffix.
        end: Vec<u8>,
    },
}

/// Exact prefix/range read permitted for a specialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageRangeAccess {
    contract_id: i32,
    domain: ResolvedStorageRangeDomain,
    direction: RangeDirection,
    options_bits: u8,
    max_items: u32,
}

impl StorageRangeAccess {
    /// Creates an exact whole-store declaration.
    #[must_use]
    pub fn whole_store(direction: RangeDirection, max_items: u32) -> Self {
        let mut options = FindOptions::None;
        options.set(FindOptions::Backwards, direction == RangeDirection::Reverse);
        Self {
            // Contract identity is carried by each row for a whole-store range.
            contract_id: 0,
            domain: ResolvedStorageRangeDomain::WholeStore,
            direction,
            options_bits: options.bits(),
            max_items,
        }
    }

    /// Creates an exact prefix declaration.
    #[must_use]
    pub fn prefix(
        contract_id: i32,
        prefix: Vec<u8>,
        direction: RangeDirection,
        mut options: FindOptions,
        max_items: u32,
    ) -> Self {
        options.set(FindOptions::Backwards, direction == RangeDirection::Reverse);
        Self {
            contract_id,
            domain: ResolvedStorageRangeDomain::Prefix(prefix),
            direction,
            options_bits: options.bits(),
            max_items,
        }
    }

    /// Creates an exact half-open range declaration.
    #[must_use]
    pub fn half_open(
        contract_id: i32,
        start: Vec<u8>,
        end: Vec<u8>,
        direction: RangeDirection,
        mut options: FindOptions,
        max_items: u32,
    ) -> Self {
        options.set(FindOptions::Backwards, direction == RangeDirection::Reverse);
        Self {
            contract_id,
            domain: ResolvedStorageRangeDomain::HalfOpen { start, end },
            direction,
            options_bits: options.bits(),
            max_items,
        }
    }

    /// Contract ID whose storage is searched.
    #[must_use]
    pub const fn contract_id(&self) -> i32 {
        self.contract_id
    }

    /// Exact resolved range domain.
    #[must_use]
    pub const fn domain(&self) -> &ResolvedStorageRangeDomain {
        &self.domain
    }

    /// Exact traversal direction.
    #[must_use]
    pub const fn direction(&self) -> RangeDirection {
        self.direction
    }

    /// Exact Neo `FindOptions`, including direction.
    #[must_use]
    pub const fn options(&self) -> FindOptions {
        FindOptions::from_bits_retain(self.options_bits)
    }

    /// Maximum accepted rows before deterministic fallback.
    #[must_use]
    pub const fn max_items(&self) -> u32 {
        self.max_items
    }
}

/// Exact storage put target and maximum value size.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageWriteAccess {
    key: StorageKey,
    max_value_bytes: usize,
}

impl StorageWriteAccess {
    /// Creates a bounded storage-put declaration.
    #[must_use]
    pub const fn new(key: StorageKey, max_value_bytes: usize) -> Self {
        Self {
            key,
            max_value_bytes,
        }
    }

    /// Exact storage key that may be inserted or replaced.
    #[must_use]
    pub const fn key(&self) -> &StorageKey {
        &self.key
    }

    /// Maximum value bytes accepted before deterministic fallback.
    #[must_use]
    pub const fn max_value_bytes(&self) -> usize {
        self.max_value_bytes
    }
}

/// Resolved native-cache entry or conservative whole-domain scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedNativeCacheScope {
    /// One exact cache-entry key after evaluating its byte expression.
    Entry(Vec<u8>),
    /// The complete versioned native-cache domain.
    WholeDomain,
}

/// Native-cache access direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCacheAccessKind {
    /// Read dependency.
    Read,
    /// Write effect.
    Write,
}

/// Exact resolved native-cache dependency or effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCacheAccess {
    domain: NativeCacheDomain,
    scope: ResolvedNativeCacheScope,
    kind: NativeCacheAccessKind,
}

impl NativeCacheAccess {
    /// Creates an exact native-cache declaration.
    #[must_use]
    pub const fn new(
        domain: NativeCacheDomain,
        scope: ResolvedNativeCacheScope,
        kind: NativeCacheAccessKind,
    ) -> Self {
        Self {
            domain,
            scope,
            kind,
        }
    }

    /// Versioned native-cache domain.
    #[must_use]
    pub const fn domain(&self) -> NativeCacheDomain {
        self.domain
    }

    /// Exact resolved entry or whole-domain scope.
    #[must_use]
    pub const fn scope(&self) -> &ResolvedNativeCacheScope {
        &self.scope
    }

    /// Read or write direction.
    #[must_use]
    pub const fn kind(&self) -> NativeCacheAccessKind {
        self.kind
    }
}

/// Host call route used by a specialized candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractCallKind {
    /// Dynamic call from a deployed script.
    Dynamic,
    /// Direct native-contract compatibility invocation.
    Native,
    /// Returning call made from a native contract frame.
    FromNativeReturning,
    /// Void call made from a native contract frame.
    FromNativeVoid,
}

/// Exact contract call shape permitted for a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractCallAccess {
    kind: ContractCallKind,
    contract: ContractResolutionIdentity,
    entry_ip: u32,
    method: String,
    call_flags_bits: u8,
    argument_count: usize,
    result_count: usize,
    native_calling_script_hash: Option<UInt160>,
}

impl ContractCallAccess {
    /// Creates an exact contract-call declaration.
    #[must_use]
    pub fn new(
        kind: ContractCallKind,
        contract: ContractResolutionIdentity,
        entry_ip: u32,
        method: impl Into<String>,
        call_flags: CallFlags,
        argument_count: usize,
        result_count: usize,
    ) -> Self {
        Self {
            kind,
            contract,
            entry_ip,
            method: method.into(),
            call_flags_bits: call_flags.bits(),
            argument_count,
            result_count,
            native_calling_script_hash: None,
        }
    }

    /// Binds the witness-visible calling hash for a call originating in a
    /// native contract frame.
    #[must_use]
    pub const fn with_native_calling_script_hash(mut self, hash: UInt160) -> Self {
        self.native_calling_script_hash = Some(hash);
        self
    }

    /// Call route.
    #[must_use]
    pub const fn kind(&self) -> ContractCallKind {
        self.kind
    }

    /// Exact target contract version.
    #[must_use]
    pub const fn contract(&self) -> ContractResolutionIdentity {
        self.contract
    }

    /// Target contract hash.
    #[must_use]
    pub const fn contract_hash(&self) -> UInt160 {
        self.contract.contract_hash()
    }

    /// Exact target entry byte offset.
    #[must_use]
    pub const fn entry_ip(&self) -> u32 {
        self.entry_ip
    }

    /// Target method.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Exact effective call flags.
    #[must_use]
    pub const fn call_flags(&self) -> CallFlags {
        CallFlags::from_bits_retain(self.call_flags_bits)
    }

    /// Exact argument count.
    #[must_use]
    pub const fn argument_count(&self) -> usize {
        self.argument_count
    }

    /// Exact child result count consumed by the candidate.
    #[must_use]
    pub const fn result_count(&self) -> usize {
        self.result_count
    }

    /// Exact native calling hash, when the route originates in a native frame.
    #[must_use]
    pub const fn native_calling_script_hash(&self) -> Option<UInt160> {
        self.native_calling_script_hash
    }
}

/// Notification identity permitted for a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationAccess {
    script_hash: UInt160,
    event_name: String,
    max_state_items: usize,
}

impl NotificationAccess {
    /// Creates an exact notification declaration.
    #[must_use]
    pub fn new(
        script_hash: UInt160,
        event_name: impl Into<String>,
        max_state_items: usize,
    ) -> Self {
        Self {
            script_hash,
            event_name: event_name.into(),
            max_state_items,
        }
    }

    /// Emitting script hash.
    #[must_use]
    pub const fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    /// Event name.
    #[must_use]
    pub fn event_name(&self) -> &str {
        &self.event_name
    }

    /// Maximum emitted state item count.
    #[must_use]
    pub const fn max_state_items(&self) -> usize {
        self.max_state_items
    }
}

/// Log identity and bounded message permitted for a candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogAccess {
    script_hash: UInt160,
    max_message_bytes: usize,
}

impl LogAccess {
    /// Creates an exact log declaration.
    #[must_use]
    pub const fn new(script_hash: UInt160, max_message_bytes: usize) -> Self {
        Self {
            script_hash,
            max_message_bytes,
        }
    }

    /// Emitting script hash.
    #[must_use]
    pub const fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    /// Maximum UTF-8 message byte count.
    #[must_use]
    pub const fn max_message_bytes(&self) -> usize {
        self.max_message_bytes
    }
}

/// External execution context input visible to a specialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostContextAccess {
    /// Execution trigger.
    Trigger,
    /// Network magic.
    Network,
    /// Address-version byte.
    AddressVersion,
    /// Current or persisting block index.
    BlockIndex,
    /// Persisting block timestamp.
    BlockTimestamp,
    /// Current script container.
    ScriptContainer,
    /// Executing script hash.
    ExecutingScriptHash,
    /// Calling script hash.
    CallingScriptHash,
    /// Entry script hash.
    EntryScriptHash,
    /// Transaction sender.
    TransactionSender,
    /// Effective call flags.
    CallFlags,
    /// Remaining gas.
    GasLeft,
    /// Whether the current context bypasses per-opcode execution fees.
    FeeWhitelist,
    /// Existing notifications visible to `GetNotifications`.
    Notifications,
    /// Applicability of one hardfork.
    Hardfork(Hardfork),
    /// Invocation count for one logical script hash.
    InvocationCounter(UInt160),
}

/// One exact host dependency or effect declared by a specialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAccessDeclaration {
    /// Exact point read, including an absent read.
    StorageRead(StorageKey),
    /// Exact prefix/range read.
    StorageRange(StorageRangeAccess),
    /// Exact storage put key. The old-value read must be declared separately.
    StorageWrite(StorageWriteAccess),
    /// Exact storage delete key.
    StorageDelete(StorageKey),
    /// Native-cache read dependency.
    NativeCacheRead(NativeCacheAccess),
    /// Native-cache write effect.
    NativeCacheWrite(NativeCacheAccess),
    /// Contract call shape.
    ContractCall(ContractCallAccess),
    /// Notification effect.
    Notification(NotificationAccess),
    /// Log effect.
    Log(LogAccess),
    /// Witness check target.
    Witness(UInt160),
    /// External context input.
    Context(HostContextAccess),
    /// Exact execution fee charge in datoshi.
    FeeCharge(u64),
    /// Exact opcode CPU-fee charge in protocol fee units.
    CpuFeeCharge(u64),
}

/// Immutable exact declaration set. An empty policy denies every host access.
#[derive(Clone, Debug)]
pub struct HostAccessPolicy {
    declarations: Arc<[HostAccessDeclaration]>,
    accounted_bytes: usize,
}

impl HostAccessPolicy {
    /// Creates one bounded immutable policy after byte-expression resolution.
    ///
    /// # Errors
    ///
    /// Returns an error before retaining a declaration beyond either limit.
    pub fn try_new(
        declarations: impl IntoIterator<Item = HostAccessDeclaration>,
        limits: HostAccessPolicyLimits,
    ) -> Result<Self, HostAccessPolicyError> {
        if limits.max_declarations == 0 {
            return Err(HostAccessPolicyError::ZeroLimit {
                limit: "max_declarations",
            });
        }
        if limits.max_bytes == 0 {
            return Err(HostAccessPolicyError::ZeroLimit { limit: "max_bytes" });
        }

        let declarations = declarations.into_iter();
        let mut retained =
            Vec::with_capacity(declarations.size_hint().0.min(limits.max_declarations));
        let mut accounted_bytes = 0usize;
        for declaration in declarations {
            if retained.len() == limits.max_declarations {
                return Err(HostAccessPolicyError::DeclarationCapacity {
                    maximum: limits.max_declarations,
                });
            }
            let required = accounted_bytes
                .saturating_add(size_of::<HostAccessDeclaration>())
                .saturating_add(declaration.dynamic_bytes());
            if required > limits.max_bytes {
                return Err(HostAccessPolicyError::ByteCapacity {
                    required,
                    maximum: limits.max_bytes,
                });
            }
            accounted_bytes = required;
            retained.push(declaration);
        }
        Ok(Self {
            declarations: retained.into(),
            accounted_bytes,
        })
    }

    /// Returns an empty deny-all policy.
    #[must_use]
    pub fn deny_all() -> Self {
        Self {
            declarations: Arc::from([]),
            accounted_bytes: 0,
        }
    }

    /// Returns the declarations in stable registry order.
    #[must_use]
    pub fn declarations(&self) -> &[HostAccessDeclaration] {
        &self.declarations
    }

    /// Deterministic retained byte accounting.
    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }

    fn allows_storage_key(
        &self,
        key: &StorageKey,
        select: impl Fn(&HostAccessDeclaration) -> Option<&StorageKey>,
    ) -> bool {
        self.declarations
            .iter()
            .filter_map(select)
            .any(|declared| declared == key)
    }

    fn allows_storage_read(&self, key: &StorageKey) -> bool {
        self.allows_storage_key(key, |access| match access {
            HostAccessDeclaration::StorageRead(key) => Some(key),
            _ => None,
        })
    }

    fn allows_storage_write(&self, key: &StorageKey, value_bytes: usize) -> bool {
        self.declarations.iter().any(|access| {
            matches!(access, HostAccessDeclaration::StorageWrite(declared)
                if declared.key() == key && value_bytes <= declared.max_value_bytes())
        })
    }

    fn allows_storage_delete(&self, key: &StorageKey) -> bool {
        self.allows_storage_key(key, |access| match access {
            HostAccessDeclaration::StorageDelete(key) => Some(key),
            _ => None,
        })
    }

    fn allows_range(&self, range: &StorageRangeAccess) -> bool {
        self.declarations.iter().any(|access| {
            matches!(access, HostAccessDeclaration::StorageRange(declared) if declared == range)
        })
    }

    fn allows_native_cache(&self, requested: &NativeCacheAccess) -> bool {
        self.declarations.iter().any(|access| {
            matches!(
                (access, requested.kind()),
                (
                    HostAccessDeclaration::NativeCacheRead(declared),
                    NativeCacheAccessKind::Read
                ) | (
                    HostAccessDeclaration::NativeCacheWrite(declared),
                    NativeCacheAccessKind::Write
                ) if declared == requested
            )
        })
    }

    fn allows_call(&self, requested: &ContractCallAccess) -> bool {
        self.declarations.iter().any(|access| {
            matches!(access, HostAccessDeclaration::ContractCall(declared) if declared == requested)
        })
    }

    fn allows_notification(
        &self,
        script_hash: UInt160,
        event_name: &str,
        state_items: usize,
    ) -> bool {
        self.declarations.iter().any(|access| {
            matches!(access, HostAccessDeclaration::Notification(declared)
                if declared.script_hash() == script_hash
                    && declared.event_name() == event_name
                    && state_items <= declared.max_state_items())
        })
    }

    fn allows_log(&self, script_hash: UInt160, message_bytes: usize) -> bool {
        self.declarations.iter().any(|access| {
            matches!(access, HostAccessDeclaration::Log(declared)
                if declared.script_hash() == script_hash
                    && message_bytes <= declared.max_message_bytes())
        })
    }

    fn allows_witness(&self, hash: UInt160) -> bool {
        self.declarations
            .contains(&HostAccessDeclaration::Witness(hash))
    }

    fn allows_context(&self, context: HostContextAccess) -> bool {
        self.declarations
            .contains(&HostAccessDeclaration::Context(context))
    }

    fn allows_fee(&self, fee: u64) -> bool {
        self.declarations
            .contains(&HostAccessDeclaration::FeeCharge(fee))
    }

    fn allows_cpu_fee(&self, fee_units: u64) -> bool {
        self.declarations
            .contains(&HostAccessDeclaration::CpuFeeCharge(fee_units))
    }
}

impl HostAccessDeclaration {
    fn dynamic_bytes(&self) -> usize {
        match self {
            Self::StorageRead(key) | Self::StorageDelete(key) => key.suffix().len(),
            Self::StorageWrite(write) => write.key().suffix().len(),
            Self::StorageRange(range) => match range.domain() {
                ResolvedStorageRangeDomain::WholeStore => 0,
                ResolvedStorageRangeDomain::Prefix(prefix) => prefix.len(),
                ResolvedStorageRangeDomain::HalfOpen { start, end } => {
                    start.len().saturating_add(end.len())
                }
            },
            Self::NativeCacheRead(access) | Self::NativeCacheWrite(access) => {
                match access.scope() {
                    ResolvedNativeCacheScope::Entry(key) => key.len(),
                    ResolvedNativeCacheScope::WholeDomain => 0,
                }
            }
            Self::ContractCall(call) => call.method().len(),
            Self::Notification(notification) => notification.event_name().len(),
            Self::Log(_)
            | Self::Witness(_)
            | Self::Context(_)
            | Self::FeeCharge(_)
            | Self::CpuFeeCharge(_) => 0,
        }
    }
}

impl Default for HostAccessPolicy {
    fn default() -> Self {
        Self::deny_all()
    }
}

/// First undeclared host access observed while running a specialization.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("undeclared specialization host access: {attempted:?}")]
pub struct HostAccessViolation {
    attempted: HostAccessDeclaration,
}

impl HostAccessViolation {
    /// Returns the exact rejected attempt.
    #[must_use]
    pub const fn attempted(&self) -> &HostAccessDeclaration {
        &self.attempted
    }
}

/// Runtime fail-closed audit for one candidate invocation.
pub struct HostAccessAudit<'policy> {
    policy: &'policy HostAccessPolicy,
    first_violation: Option<HostAccessViolation>,
}

impl<'policy> HostAccessAudit<'policy> {
    /// Starts a clean audit against an immutable policy.
    #[must_use]
    pub const fn new(policy: &'policy HostAccessPolicy) -> Self {
        Self {
            policy,
            first_violation: None,
        }
    }

    /// Returns whether no undeclared access has occurred.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.first_violation.is_none()
    }

    /// Returns the first violation, if any.
    #[must_use]
    pub const fn violation(&self) -> Option<&HostAccessViolation> {
        self.first_violation.as_ref()
    }

    /// Completes the audit, returning the latched violation on failure.
    pub fn finish(self) -> Result<(), HostAccessViolation> {
        match self.first_violation {
            Some(violation) => Err(violation),
            None => Ok(()),
        }
    }

    fn require(
        &mut self,
        permitted: impl FnOnce(&HostAccessPolicy) -> bool,
        attempted: impl FnOnce() -> HostAccessDeclaration,
    ) -> Result<(), HostAccessViolation> {
        if let Some(violation) = &self.first_violation {
            return Err(violation.clone());
        }
        if permitted(self.policy) {
            return Ok(());
        }
        let violation = HostAccessViolation {
            attempted: attempted(),
        };
        self.first_violation = Some(violation.clone());
        Err(violation)
    }

    pub(crate) fn storage_read(&mut self, key: &StorageKey) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_storage_read(key),
            || HostAccessDeclaration::StorageRead(key.clone()),
        )
    }

    pub(crate) fn storage_range(
        &mut self,
        range: &StorageRangeAccess,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_range(range),
            || HostAccessDeclaration::StorageRange(range.clone()),
        )
    }

    pub(crate) fn storage_write(
        &mut self,
        key: &StorageKey,
        value_bytes: usize,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_storage_write(key, value_bytes),
            || {
                HostAccessDeclaration::StorageWrite(StorageWriteAccess::new(
                    key.clone(),
                    value_bytes,
                ))
            },
        )
    }

    pub(crate) fn storage_delete(&mut self, key: &StorageKey) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_storage_delete(key),
            || HostAccessDeclaration::StorageDelete(key.clone()),
        )
    }

    /// Authorizes one resolved native-cache read or write.
    ///
    /// This does not expose or perform cache access. A typed native-cache
    /// adapter must call it before touching its declared domain and scope.
    pub fn authorize_native_cache(
        &mut self,
        access: &NativeCacheAccess,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_native_cache(access),
            || match access.kind() {
                NativeCacheAccessKind::Read => {
                    HostAccessDeclaration::NativeCacheRead(access.clone())
                }
                NativeCacheAccessKind::Write => {
                    HostAccessDeclaration::NativeCacheWrite(access.clone())
                }
            },
        )
    }

    pub(crate) fn contract_call(
        &mut self,
        access: &ContractCallAccess,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_call(access),
            || HostAccessDeclaration::ContractCall(access.clone()),
        )
    }

    pub(crate) fn notification(
        &mut self,
        script_hash: UInt160,
        event_name: &str,
        state_items: usize,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_notification(script_hash, event_name, state_items),
            || {
                HostAccessDeclaration::Notification(NotificationAccess::new(
                    script_hash,
                    event_name,
                    state_items,
                ))
            },
        )
    }

    pub(crate) fn log(
        &mut self,
        script_hash: UInt160,
        message_bytes: usize,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_log(script_hash, message_bytes),
            || HostAccessDeclaration::Log(LogAccess::new(script_hash, message_bytes)),
        )
    }

    pub(crate) fn witness(&mut self, hash: UInt160) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_witness(hash),
            || HostAccessDeclaration::Witness(hash),
        )
    }

    pub(crate) fn context(
        &mut self,
        context: HostContextAccess,
    ) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_context(context),
            || HostAccessDeclaration::Context(context),
        )
    }

    pub(crate) fn fee(&mut self, fee: u64) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_fee(fee),
            || HostAccessDeclaration::FeeCharge(fee),
        )
    }

    pub(crate) fn cpu_fee(&mut self, fee_units: u64) -> Result<(), HostAccessViolation> {
        self.require(
            |policy| policy.allows_cpu_fee(fee_units),
            || HostAccessDeclaration::CpuFeeCharge(fee_units),
        )
    }
}

#[cfg(test)]
#[path = "../tests/host_access_audit/policy.rs"]
mod tests;
