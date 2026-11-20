//! Execution-strategy selection and cross-strategy equivalence verification.
//!
//! Block persistence has to decide *how* an application script runs, not only
//! *that* it runs. Those decisions are execution semantics, so this module owns
//! them instead of the node orchestration layer:
//!
//! - [`ExecutionMode`]: the operator-visible policy (`canonical`, `auto`,
//!   `verify`) together with its stable configuration strings and numeric codes.
//! - [`ExecutionStrategy`]: the per-script decision actually taken after the
//!   fast-path eligibility checks.
//! - [`ExecutionFingerprint`]: the semantic projection of one finished result,
//!   used to prove that direct native dispatch and the canonical interpreter
//!   agree.
//! - [`execute_application_script`]: the fallback orchestration that turns an
//!   [`ExecutionRequest`] into an [`ExecutionReport`].
//!
//! # Dependency direction
//!
//! This module names no ledger type. The ledger-facing orchestration supplies
//! the two capabilities it needs through ports:
//! [`CanonicalVerificationSource`] builds the isolated canonical engine used by
//! [`ExecutionMode::Verify`], and [`ExecutionStrategyObserver`] receives the
//! direct-dispatch counter.

use super::ApplicationEngine;
use super::fast_path::Nep17Parser;
use neo_primitives::UInt160;
use neo_vm::StackItem;
use neo_vm::VmState;
use std::fmt;
use std::str::FromStr;

pub use super::fast_path::TransferParams;

/// Parses the strict NEP-17 transfer template out of `script`.
///
/// A `None` return means the script is not a candidate at all, so neither
/// [`ExecutionMode::Auto`] nor [`ExecutionMode::Verify`] will consider direct
/// dispatch for it.
pub fn parse_fast_path_candidate(script: &[u8]) -> Option<TransferParams> {
    Nep17Parser::parse_transfer_script(script).ok()
}

/// Selects the transaction execution engine used by block persistence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ExecutionMode {
    /// Always execute through the canonical VM interpreter.
    #[default]
    Canonical,
    /// Use direct native dispatch only after strict script and state eligibility checks.
    Auto,
    /// Dual-execute eligible transfers on isolated snapshots and reject any divergence.
    Verify,
}

impl ExecutionMode {
    /// Every mode, in configuration order.
    ///
    /// Telemetry emits one `mode` label per entry, so this list is the single
    /// source of truth for the bounded label set.
    pub const ALL: [Self; 3] = [Self::Canonical, Self::Auto, Self::Verify];

    /// Returns the stable configuration value for this mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Auto => "auto",
            Self::Verify => "verify",
        }
    }

    /// Returns the stable numeric code published to atomically swapped state.
    pub const fn code(self) -> u8 {
        match self {
            Self::Canonical => 0,
            Self::Auto => 1,
            Self::Verify => 2,
        }
    }

    /// Decodes a numeric code, falling back to [`Self::Canonical`].
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Auto,
            2 => Self::Verify,
            _ => Self::Canonical,
        }
    }

    /// Returns whether this mode needs an isolated [`CanonicalVerificationSource`].
    ///
    /// Callers use this to avoid paying for snapshot isolation and a second
    /// engine in modes that never dual-execute.
    pub const fn requires_isolated_source(self) -> bool {
        matches!(self, Self::Verify)
    }
}

impl FromStr for ExecutionMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "canonical" => Ok(Self::Canonical),
            "auto" => Ok(Self::Auto),
            "verify" => Ok(Self::Verify),
            other => Err(format!(
                "unsupported execution mode '{other}'; expected canonical, auto, or verify"
            )),
        }
    }
}

impl fmt::Display for ExecutionMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The interpreter actually selected for one application script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionStrategy {
    /// The canonical VM interpreter ran the script.
    Canonical,
    /// Direct native dispatch executed a strictly parsed NEP-17 transfer.
    DirectTransfer,
}

impl ExecutionStrategy {
    /// Returns whether direct native dispatch ran the script.
    pub const fn is_direct(self) -> bool {
        matches!(self, Self::DirectTransfer)
    }
}

/// Counter sink for direct-dispatch attempts and eligibility declines.
///
/// Implementations must not mutate execution state; this is observability only.
pub trait ExecutionStrategyObserver: Send + Sync {
    /// Records one attempt that ran through direct native dispatch.
    ///
    /// The attempt may end in `HALT` or in any canonical fault: a faulted
    /// dispatch still consumed the fast path and still produced a receipt. A
    /// transfer declined by an eligibility check never reaches this call.
    fn record_direct_transfer_attempt(&self);

    /// Records one parsed NEP-17 transfer that failed the eligibility checks.
    ///
    /// Only a script that matched the transfer template and then failed an
    /// engine-side check (call flags, invoking state, native registration,
    /// activity, participant kind, or fixed-fee whitelisting) reaches this
    /// call. Scripts that were never transfer candidates do not: they were not
    /// declined, they were simply not eligible candidates at all.
    fn record_fast_path_decline(&self);
}

/// Builds the isolated canonical engine used to verify a direct dispatch.
///
/// The source executes the same script against its own fork of the snapshot so
/// that neither result observes the other's writes.
pub trait CanonicalVerificationSource {
    /// Executes `script` canonically on an isolated snapshot and returns the
    /// finished engine so the caller can capture its semantic fingerprint.
    fn execute_canonical(&self, script: &[u8]) -> Result<ApplicationEngine, String>;
}

/// Semantic projection of a finished application-engine result.
///
/// Two strategies agree exactly when their fingerprints are equal; the
/// projection therefore covers every consensus-visible output of execution and
/// deliberately ignores host-side bookkeeping such as profiler counters.
#[derive(Debug, PartialEq, Eq)]
pub struct ExecutionFingerprint {
    pub state: VmState,
    pub fee_consumed: i64,
    pub exception: Option<String>,
    pub result_stack: Vec<StackItem>,
    pub notifications: Vec<(UInt160, String, Vec<StackItem>)>,
    pub logs: Vec<(UInt160, String)>,
    pub storage: Vec<(Vec<u8>, u8, Vec<u8>)>,
}

impl ExecutionFingerprint {
    /// Captures the semantic state of `engine`.
    pub fn capture(engine: &ApplicationEngine) -> Self {
        let mut storage: Vec<_> = engine
            .original_snapshot_cache()
            .tracked_items()
            .into_iter()
            .map(|(key, entry)| (key.to_array(), entry.state as u8, entry.item.to_value()))
            .collect();
        storage.sort_by(|left, right| left.0.cmp(&right.0));
        Self {
            state: engine.state(),
            fee_consumed: engine.fee_consumed(),
            exception: engine.fault_exception().map(str::to_owned),
            result_stack: engine.result_stack().to_vec(),
            notifications: engine
                .notifications()
                .iter()
                .map(|event| {
                    (
                        event.script_hash,
                        event.event_name.clone(),
                        event.state.clone(),
                    )
                })
                .collect(),
            logs: engine
                .logs()
                .iter()
                .map(|event| (event.script_hash, event.message.clone()))
                .collect(),
            storage,
        }
    }

    /// Returns the divergence message when `expected` and `actual` differ.
    ///
    /// A `None` return is the only accepted outcome of dual execution.
    pub fn divergence(expected: &Self, actual: &Self) -> Option<String> {
        (expected != actual).then(|| {
            format!("direct/canonical execution divergence: expected {expected:?}, got {actual:?}")
        })
    }
}

/// Inputs for one application-script execution decision.
///
/// Only [`mode`](Self::mode) is always required: `auto` needs a parsed transfer
/// to consider direct dispatch, and `verify` additionally needs the script bytes
/// and an isolated canonical source.
pub struct ExecutionRequest<'a> {
    /// Operator-selected policy for this execution.
    pub mode: ExecutionMode,
    /// Parsed strict NEP-17 transfer, when the loaded script matched the template.
    pub transfer: Option<&'a TransferParams>,
    /// Loaded script bytes; read only by [`ExecutionMode::Verify`].
    pub script: Option<&'a [u8]>,
    /// Isolated canonical engine source; read only by [`ExecutionMode::Verify`].
    pub canonical_source: Option<&'a dyn CanonicalVerificationSource>,
    /// Counter sink for direct-dispatch attempts.
    pub observer: Option<&'a dyn ExecutionStrategyObserver>,
}

impl<'a> ExecutionRequest<'a> {
    /// Creates a request for `mode` with no optional capability attached.
    pub const fn new(mode: ExecutionMode) -> Self {
        Self {
            mode,
            transfer: None,
            script: None,
            canonical_source: None,
            observer: None,
        }
    }

    /// Attaches the parsed NEP-17 transfer candidate.
    pub const fn with_transfer(mut self, transfer: Option<&'a TransferParams>) -> Self {
        self.transfer = transfer;
        self
    }

    /// Attaches the loaded script bytes.
    pub const fn with_script(mut self, script: Option<&'a [u8]>) -> Self {
        self.script = script;
        self
    }

    /// Attaches the isolated canonical engine source.
    pub const fn with_canonical_source(
        mut self,
        source: Option<&'a dyn CanonicalVerificationSource>,
    ) -> Self {
        self.canonical_source = source;
        self
    }

    /// Attaches the direct-dispatch counter sink.
    pub const fn with_observer(
        mut self,
        observer: Option<&'a dyn ExecutionStrategyObserver>,
    ) -> Self {
        self.observer = observer;
        self
    }
}

/// Result of executing one application script under an [`ExecutionMode`].
#[derive(Debug)]
pub struct ExecutionReport {
    /// VM state produced by the strategy that actually ran.
    pub state: VmState,
    /// Interpreter that actually ran.
    pub strategy: ExecutionStrategy,
    /// Dual-execution verification failure, if any.
    pub verification_error: Option<String>,
}

impl ExecutionReport {
    /// Returns the verification outcome as a ledger-facing result.
    pub fn verify(&self) -> Result<(), String> {
        match &self.verification_error {
            Some(message) => Err(message.clone()),
            None => Ok(()),
        }
    }
}

/// Executes `engine`'s loaded script according to `request`.
///
/// The function never retries a failed direct dispatch: once direct dispatch
/// starts, its errors become canonical VM faults and the caller applies the
/// normal transaction rollback rules.
pub fn execute_application_script(
    engine: &mut ApplicationEngine,
    request: &ExecutionRequest<'_>,
) -> ExecutionReport {
    match request.mode {
        ExecutionMode::Canonical => canonical_only(engine),
        ExecutionMode::Auto => run_direct_or_canonical(engine, request),
        ExecutionMode::Verify => run_verified(engine, request),
    }
}

/// Executes with no fast-path eligibility at all.
fn canonical_only(engine: &mut ApplicationEngine) -> ExecutionReport {
    ExecutionReport {
        state: engine.execute_allow_fault(),
        strategy: ExecutionStrategy::Canonical,
        verification_error: None,
    }
}

/// Falls back to the canonical interpreter while reporting why.
fn canonical_fallback(
    engine: &mut ApplicationEngine,
    message: impl Into<String>,
) -> ExecutionReport {
    ExecutionReport {
        state: engine.execute_allow_fault(),
        strategy: ExecutionStrategy::Canonical,
        verification_error: Some(message.into()),
    }
}

/// Returns the transfer only when every eligibility check passes.
///
/// A template match that fails an engine-side check is reported to the observer
/// as a decline; a script that never matched the template is not.
fn eligible_transfer<'a>(
    engine: &mut ApplicationEngine,
    request: &'a ExecutionRequest<'_>,
) -> Option<&'a TransferParams> {
    let params = request.transfer?;
    if engine.can_execute_direct_nep17_transfer(params) {
        return Some(params);
    }
    if let Some(observer) = request.observer {
        observer.record_fast_path_decline();
    }
    None
}

/// `ExecutionMode::Auto`: direct dispatch when eligible, canonical otherwise.
fn run_direct_or_canonical(
    engine: &mut ApplicationEngine,
    request: &ExecutionRequest<'_>,
) -> ExecutionReport {
    let Some(params) = eligible_transfer(engine, request) else {
        return canonical_only(engine);
    };
    let state = engine.execute_direct_nep17_transfer(params);
    // Recorded after the dispatch finishes, so the counter reports attempts that
    // actually ran rather than attempts that were about to start.
    if let Some(observer) = request.observer {
        observer.record_direct_transfer_attempt();
    }
    ExecutionReport {
        state,
        strategy: ExecutionStrategy::DirectTransfer,
        verification_error: None,
    }
}

/// `ExecutionMode::Verify`: dual execution with mandatory fingerprint equality.
fn run_verified(engine: &mut ApplicationEngine, request: &ExecutionRequest<'_>) -> ExecutionReport {
    if eligible_transfer(engine, request).is_none() {
        return canonical_only(engine);
    }
    let Some(source) = request.canonical_source else {
        return canonical_fallback(engine, "missing isolated verification context");
    };
    let Some(script) = request.script else {
        return canonical_fallback(engine, "missing script for isolated verification");
    };
    let canonical = match source.execute_canonical(script) {
        Ok(canonical) => canonical,
        Err(error) => {
            return canonical_fallback(
                engine,
                format!("failed to construct isolated canonical verification: {error}"),
            );
        }
    };
    let direct = run_direct_or_canonical(engine, request);
    let expected = ExecutionFingerprint::capture(&canonical);
    let actual = ExecutionFingerprint::capture(engine);
    // Keep the canonical engine alive through fingerprint capture; its isolated
    // snapshot is never committed into the ledger overlay.
    drop(canonical);
    ExecutionReport {
        state: direct.state,
        strategy: direct.strategy,
        verification_error: ExecutionFingerprint::divergence(&expected, &actual),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_mode_round_trips_through_str_and_code() {
        for mode in [
            ExecutionMode::Canonical,
            ExecutionMode::Auto,
            ExecutionMode::Verify,
        ] {
            assert_eq!(mode.as_str().parse::<ExecutionMode>(), Ok(mode));
            assert_eq!(ExecutionMode::from_code(mode.code()), mode);
        }
        assert_eq!(ExecutionMode::default(), ExecutionMode::Canonical);
        assert_eq!(ExecutionMode::from_code(9), ExecutionMode::Canonical);
        assert!(!ExecutionMode::Canonical.requires_isolated_source());
        assert!(!ExecutionMode::Auto.requires_isolated_source());
        assert!(ExecutionMode::Verify.requires_isolated_source());
    }

    #[test]
    fn execution_mode_rejects_unknown_names() {
        let error = "turbo".parse::<ExecutionMode>().unwrap_err();
        assert!(error.contains("unsupported execution mode 'turbo'"));
    }

    #[test]
    fn execution_mode_display_matches_configuration_value() {
        assert_eq!(ExecutionMode::Auto.to_string(), "auto");
        assert_eq!(ExecutionMode::Verify.to_string(), "verify");
    }

    #[test]
    fn strategy_reports_direct_dispatch() {
        assert!(!ExecutionStrategy::Canonical.is_direct());
        assert!(ExecutionStrategy::DirectTransfer.is_direct());
    }

    #[test]
    fn report_surfaces_verification_failure() {
        let report = ExecutionReport {
            state: VmState::HALT,
            strategy: ExecutionStrategy::DirectTransfer,
            verification_error: Some("boom".to_string()),
        };
        assert_eq!(report.verify(), Err("boom".to_string()));
        assert_eq!(
            ExecutionReport {
                verification_error: None,
                ..report
            }
            .verify(),
            Ok(())
        );
    }
}
