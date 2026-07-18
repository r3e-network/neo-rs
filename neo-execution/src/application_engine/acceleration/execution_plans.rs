use super::super::*;
use neo_vm::{
    ContractResolutionIdentity, ExecutionPlan, ExecutionPlanCache, ExecutionPlanCacheError,
    ExecutionPlanCacheLimits, ExecutionPlanCacheSnapshot, ExecutionPlanKey, ExecutionPlanLimits,
    HardforkPlanState, HardforkTableIdentity, ProtocolIdentity, ProtocolVersion,
};

/// Disabled-by-default configuration for immutable NeoVM execution plans.
///
/// Plans cache decoded execution structure only. They never cache stacks,
/// storage, gas, faults, or any other execution result.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ApplicationExecutionPlanConfig {
    /// Execute every script through the ordinary NeoVM path and allocate no
    /// application-level plan cache.
    #[default]
    Disabled,
    /// Attach exact, immutable plans using the supplied hard bounds.
    Enabled {
        /// Maximum cache residency and in-flight construction.
        cache_limits: ExecutionPlanCacheLimits,
        /// Maximum work and memory accepted by one plan build.
        plan_limits: ExecutionPlanLimits,
    },
}

impl ApplicationExecutionPlanConfig {
    /// Enables plans with the conservative bounded NeoVM defaults.
    pub const DEFAULT_ENABLED: Self = Self::Enabled {
        cache_limits: ExecutionPlanCacheLimits::DEFAULT,
        plan_limits: ExecutionPlanLimits::DEFAULT,
    };

    /// Returns whether plan construction and routing are explicitly enabled.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled { .. })
    }
}

/// Shareable bounded immutable-plan cache for explicitly configured engines.
///
/// Cloning this handle shares only decoded plans and their bounded counters.
/// It does not share VM sessions, stacks, storage, effects, or execution
/// results. A long-lived node service can therefore reuse warm plans across
/// short-lived per-block application engines without a global cache.
#[derive(Clone, Debug)]
pub struct ApplicationExecutionPlanCache {
    config: ApplicationExecutionPlanConfig,
    inner: Arc<ExecutionPlanCache>,
}

impl ApplicationExecutionPlanCache {
    /// Creates an empty shareable cache with explicit hard bounds.
    #[must_use]
    pub fn new(cache_limits: ExecutionPlanCacheLimits, plan_limits: ExecutionPlanLimits) -> Self {
        Self {
            config: ApplicationExecutionPlanConfig::Enabled {
                cache_limits,
                plan_limits,
            },
            inner: Arc::new(ExecutionPlanCache::new(cache_limits, plan_limits)),
        }
    }

    /// Returns the enabled bounds carried by this cache.
    #[must_use]
    pub const fn config(&self) -> ApplicationExecutionPlanConfig {
        self.config
    }

    /// Returns current bounded counters without exposing exact plan keys.
    #[must_use]
    pub fn snapshot(&self) -> ExecutionPlanCacheSnapshot {
        self.inner.snapshot()
    }
}

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    /// Reconfigures the bounded immutable-plan cache.
    ///
    /// Changing configuration discards only rebuildable plans. Execution
    /// results and application state are never stored in this cache.
    pub fn set_execution_plan_config(&mut self, config: ApplicationExecutionPlanConfig) {
        if self.execution_plan_config() == config {
            return;
        }

        self.execution_plan_cache = match config {
            ApplicationExecutionPlanConfig::Disabled => None,
            ApplicationExecutionPlanConfig::Enabled {
                cache_limits,
                plan_limits,
            } => Some(ApplicationExecutionPlanCache::new(
                cache_limits,
                plan_limits,
            )),
        };
    }

    /// Installs a bounded cache that may be shared with other application engines.
    ///
    /// This is still an explicit opt-in. Passing a handle cannot enable any
    /// specialization or cache execution outputs; it only enables immutable
    /// plan lookup for contexts loaded after this call.
    pub fn set_shared_execution_plan_cache(&mut self, cache: ApplicationExecutionPlanCache) {
        self.execution_plan_cache = Some(cache);
    }

    /// Returns a cloneable handle to the enabled bounded cache.
    #[must_use]
    pub fn shared_execution_plan_cache(&self) -> Option<ApplicationExecutionPlanCache> {
        self.execution_plan_cache.clone()
    }

    /// Returns the current immutable-plan routing configuration.
    #[must_use]
    pub fn execution_plan_config(&self) -> ApplicationExecutionPlanConfig {
        self.execution_plan_cache
            .as_ref()
            .map_or(ApplicationExecutionPlanConfig::Disabled, |cache| {
                cache.config()
            })
    }

    /// Returns bounded cache counters when immutable plans are enabled.
    #[must_use]
    pub fn execution_plan_cache_snapshot(&self) -> Option<ExecutionPlanCacheSnapshot> {
        self.execution_plan_cache
            .as_ref()
            .map(ApplicationExecutionPlanCache::snapshot)
    }

    pub(in crate::application_engine) fn hardfork_plan_identity(&self) -> HardforkTableIdentity {
        let current_index = self.current_block_index();
        Hardfork::ALL
            .into_iter()
            .fold(HardforkTableIdentity::unconfigured(), |table, hardfork| {
                let state = match self.protocol_settings.hardforks.get(&hardfork).copied() {
                    None => HardforkPlanState::Unconfigured,
                    Some(activation_height) if current_index >= activation_height => {
                        HardforkPlanState::Active { activation_height }
                    }
                    Some(activation_height) => HardforkPlanState::Pending { activation_height },
                };
                table.with_state(hardfork, state)
            })
    }

    pub(in crate::application_engine) fn execution_plan_key(
        &self,
        script: &Script,
        initial_position: usize,
        contract: Option<&ContractState>,
    ) -> Option<ExecutionPlanKey> {
        let entry_ip = u32::try_from(initial_position).ok()?;
        let contract = contract.map(|contract| {
            ContractResolutionIdentity::new(
                contract.hash,
                contract.id,
                contract.update_counter,
                contract.nef.checksum,
            )
        });

        Some(ExecutionPlanKey::new(
            script.shared_bytes(),
            entry_ip,
            ProtocolIdentity::new(
                self.protocol_settings.network,
                ProtocolVersion::NEO_N3_V3_10_1,
            ),
            self.hardfork_plan_identity(),
            self.trigger,
            contract,
        ))
    }

    fn attach_execution_plan_or_fallback(
        script: Script,
        initial_position: usize,
        plan: Result<Arc<ExecutionPlan>, ExecutionPlanCacheError>,
    ) -> Script {
        let Some(plan) = plan.ok() else {
            return script;
        };
        let Some(entry_ip) = u32::try_from(initial_position).ok() else {
            return script;
        };
        if plan.key().entry_ip() != entry_ip
            || !plan.matches_script(&script.script_hash(), script.as_bytes())
        {
            return script;
        }

        script.clone().with_execution_plan(plan).unwrap_or(script)
    }

    #[inline]
    pub(in crate::application_engine) fn prepare_script_with_execution_plan(
        &self,
        script: Script,
        initial_position: usize,
        contract: Option<&ContractState>,
    ) -> Script {
        let Some(cache) = self.execution_plan_cache.as_ref() else {
            return script;
        };
        if script.execution_plan().is_some() {
            return script;
        }
        let Some(key) = self.execution_plan_key(&script, initial_position, contract) else {
            return script;
        };
        let plan = cache.inner.get_or_build(key);
        Self::attach_execution_plan_or_fallback(script, initial_position, plan)
    }
}

#[cfg(test)]
#[path = "../../tests/application_engine/execution_plans.rs"]
mod tests;
