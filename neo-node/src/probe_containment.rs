//! Fail-closed platform capability detection for the restricted probe worker.

#[cfg(target_os = "linux")]
use std::path::Path;

/// Identifier for the only supported restricted probe containment profile.
pub(crate) const RESTRICTED_PROBE_PROFILE: &str = "neo-probe-restricted-v1";
/// Required per-worker memory ceiling.
pub(crate) const REQUIRED_MEMORY_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum wall-clock duration for one replay.
pub(crate) const MAX_RUN_TIMEOUT_MS: u64 = 3_000;
/// Maximum wall-clock duration for the complete replay suite.
pub(crate) const MAX_SUITE_TIMEOUT_MS: u64 = 7_000;
/// Maximum admitted work queue size.
pub(crate) const MAX_QUEUE_CAPACITY: usize = 16;
/// Restricted probe execution is deliberately single-process and serial.
pub(crate) const REQUIRED_CONCURRENCY: usize = 1;

/// Exact containment contract requested by the probe controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeContainmentProfile {
    /// Stable profile identifier.
    pub(crate) name: &'static str,
    /// Exact memory ceiling enforced for the worker process.
    pub(crate) memory_limit_bytes: u64,
    /// Per-run wall/CPU budget.
    pub(crate) run_timeout_ms: u64,
    /// Whole-suite wall-clock budget.
    pub(crate) suite_timeout_ms: u64,
}

impl ProbeContainmentProfile {
    /// Builds the fixed profile for capability contract tests.
    #[cfg(test)]
    pub(crate) const fn restricted() -> Self {
        Self {
            name: RESTRICTED_PROBE_PROFILE,
            memory_limit_bytes: REQUIRED_MEMORY_LIMIT_BYTES,
            run_timeout_ms: MAX_RUN_TIMEOUT_MS,
            suite_timeout_ms: MAX_SUITE_TIMEOUT_MS,
        }
    }
}

/// Individual properties that must all be proven before the worker can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainmentProperty {
    /// Descendants are terminated and reaped with the worker.
    ProcessTreeKillAndReap,
    /// The exact 256 MiB memory ceiling is enforced.
    MemoryLimit,
    /// CPU and wall-clock deadlines are enforced.
    CpuAndDeadline,
    /// No ambient privileged handles or file descriptors are inherited.
    NoPrivilegedHandleInheritance,
    /// The worker receives an empty environment.
    EmptyEnvironment,
    /// The configured executable is fixed after validation.
    FixedExecutable,
    /// No caller-controlled arguments, paths, or URLs are accepted.
    NoArbitraryArgumentsPathsOrUrls,
    /// Network access is denied by a hard platform boundary.
    NetworkIsolation,
    /// Filesystem access is denied by a hard platform boundary.
    FilesystemIsolation,
    /// Thread creation/escape is denied by a hard platform boundary.
    ThreadIsolation,
}

/// Platform evidence for a requested containment profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeContainmentEvidence {
    /// Profile for which this evidence was collected.
    pub(crate) profile: ProbeContainmentProfile,
    /// Properties that the provider can prove.
    pub(crate) proven: Vec<ContainmentProperty>,
    /// Required properties that remain unproven.
    pub(crate) missing: Vec<ContainmentProperty>,
    /// Human-readable platform mechanism summary for startup diagnostics.
    pub(crate) mechanism: &'static str,
}

impl ProbeContainmentEvidence {
    fn from_proven(
        profile: ProbeContainmentProfile,
        proven: Vec<ContainmentProperty>,
        mechanism: &'static str,
    ) -> Self {
        let required = [
            ContainmentProperty::ProcessTreeKillAndReap,
            ContainmentProperty::MemoryLimit,
            ContainmentProperty::CpuAndDeadline,
            ContainmentProperty::NoPrivilegedHandleInheritance,
            ContainmentProperty::EmptyEnvironment,
            ContainmentProperty::FixedExecutable,
            ContainmentProperty::NoArbitraryArgumentsPathsOrUrls,
            ContainmentProperty::NetworkIsolation,
            ContainmentProperty::FilesystemIsolation,
            ContainmentProperty::ThreadIsolation,
        ];
        let missing = required
            .into_iter()
            .filter(|property| !proven.contains(property))
            .collect();
        Self {
            profile,
            proven,
            missing,
            mechanism,
        }
    }

    /// Returns true only when every required property is proven.
    pub(crate) fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Result of platform capability detection. There is no permissive fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeContainmentCapability {
    /// Every property of the exact requested profile is proven.
    #[allow(dead_code)]
    // Reserved for a future native provider that proves every hard boundary.
    Supported(ProbeContainmentEvidence),
    /// At least one mandatory property remains unproven.
    Unsupported(ProbeContainmentEvidence),
}

impl ProbeContainmentCapability {
    /// Returns evidence regardless of whether the capability is complete.
    #[cfg(test)]
    pub(crate) const fn evidence(&self) -> &ProbeContainmentEvidence {
        match self {
            Self::Supported(evidence) | Self::Unsupported(evidence) => evidence,
        }
    }

    /// Constructs a trusted capability for narrow unit-test injection.
    #[cfg(test)]
    pub(crate) fn trusted_for_test(profile: ProbeContainmentProfile) -> Self {
        let proven = vec![
            ContainmentProperty::ProcessTreeKillAndReap,
            ContainmentProperty::MemoryLimit,
            ContainmentProperty::CpuAndDeadline,
            ContainmentProperty::NoPrivilegedHandleInheritance,
            ContainmentProperty::EmptyEnvironment,
            ContainmentProperty::FixedExecutable,
            ContainmentProperty::NoArbitraryArgumentsPathsOrUrls,
            ContainmentProperty::NetworkIsolation,
            ContainmentProperty::FilesystemIsolation,
            ContainmentProperty::ThreadIsolation,
        ];
        Self::Supported(ProbeContainmentEvidence::from_proven(
            profile,
            proven,
            "trusted-test-provider",
        ))
    }
}

/// Injected source of platform containment evidence.
pub(crate) trait ProbeContainmentCapabilityProvider: Send + Sync {
    /// Evaluates support for the exact requested profile.
    fn capability(&self, profile: &ProbeContainmentProfile) -> ProbeContainmentCapability;
}

/// Native fail-closed platform detector.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NativeProbeContainment;

impl ProbeContainmentCapabilityProvider for NativeProbeContainment {
    fn capability(&self, profile: &ProbeContainmentProfile) -> ProbeContainmentCapability {
        native_capability(profile.clone())
    }
}

/// Fixed capability provider used to inject independently trusted evidence.
#[cfg(test)]
pub(crate) struct FixedProbeContainment(pub(crate) ProbeContainmentCapability);

#[cfg(test)]
impl ProbeContainmentCapabilityProvider for FixedProbeContainment {
    fn capability(&self, _profile: &ProbeContainmentProfile) -> ProbeContainmentCapability {
        self.0.clone()
    }
}

#[cfg(windows)]
fn native_capability(profile: ProbeContainmentProfile) -> ProbeContainmentCapability {
    // Job Objects can enforce tree lifetime, active-process, CPU-time, and memory
    // limits. Rust's process launcher can also clear the environment and pass only
    // the three protocol pipes. Windows does not provide this profile with a
    // proven network, filesystem, and thread-creation hard boundary, so the full
    // capability must remain unsupported.
    let evidence = ProbeContainmentEvidence::from_proven(
        profile,
        vec![
            ContainmentProperty::ProcessTreeKillAndReap,
            ContainmentProperty::MemoryLimit,
            ContainmentProperty::CpuAndDeadline,
            ContainmentProperty::NoPrivilegedHandleInheritance,
            ContainmentProperty::EmptyEnvironment,
            ContainmentProperty::FixedExecutable,
            ContainmentProperty::NoArbitraryArgumentsPathsOrUrls,
        ],
        "windows-job-object-partial",
    );
    ProbeContainmentCapability::Unsupported(evidence)
}

#[cfg(target_os = "linux")]
fn native_capability(profile: ProbeContainmentProfile) -> ProbeContainmentCapability {
    let namespace_support = Path::new("/proc/self/ns/user").exists()
        && Path::new("/proc/self/ns/pid").exists()
        && Path::new("/proc/self/ns/net").exists()
        && Path::new("/proc/self/ns/mnt").exists();
    let cgroup_v2_support = Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    let seccomp_support = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("Seccomp:")
                    .and_then(|value| value.trim().parse::<u8>().ok())
            })
        })
        .is_some_and(|mode| mode > 0);

    let mut proven = vec![
        ContainmentProperty::EmptyEnvironment,
        ContainmentProperty::FixedExecutable,
        ContainmentProperty::NoArbitraryArgumentsPathsOrUrls,
    ];
    if namespace_support {
        proven.push(ContainmentProperty::NetworkIsolation);
        proven.push(ContainmentProperty::FilesystemIsolation);
        proven.push(ContainmentProperty::NoPrivilegedHandleInheritance);
    }
    if cgroup_v2_support {
        proven.push(ContainmentProperty::ProcessTreeKillAndReap);
        proven.push(ContainmentProperty::MemoryLimit);
        proven.push(ContainmentProperty::CpuAndDeadline);
    }
    if seccomp_support {
        proven.push(ContainmentProperty::ThreadIsolation);
    }
    let evidence = ProbeContainmentEvidence::from_proven(
        profile,
        proven,
        "linux-namespaces-cgroup-v2-seccomp-detection",
    );
    // Detection proves host primitives exist, not that this node has installed a
    // race-free worker sandbox. Keep startup closed until that launcher exists.
    ProbeContainmentCapability::Unsupported(evidence)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn native_capability(profile: ProbeContainmentProfile) -> ProbeContainmentCapability {
    ProbeContainmentCapability::Unsupported(ProbeContainmentEvidence::from_proven(
        profile,
        Vec::new(),
        "unsupported-platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_capability_is_complete_for_exact_profile() {
        let profile = ProbeContainmentProfile::restricted();
        let capability = ProbeContainmentCapability::trusted_for_test(profile.clone());
        assert!(matches!(
            capability,
            ProbeContainmentCapability::Supported(_)
        ));
        assert_eq!(capability.evidence().profile, profile);
        assert!(capability.evidence().is_complete());
    }

    #[test]
    fn native_capability_never_claims_an_incomplete_profile() {
        let capability = NativeProbeContainment.capability(&ProbeContainmentProfile::restricted());
        if let ProbeContainmentCapability::Supported(evidence) = capability {
            assert!(evidence.is_complete());
        }
    }
}
