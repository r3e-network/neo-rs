use crate::native_contract_provider::NativeContractProvider;
use crate::{NativeContract, NativeMethod};
use neo_config::ProtocolSettings;
use neo_error::CoreError;
use neo_error::CoreResult;
use neo_primitives::UInt160;
use std::collections::HashMap;
use std::sync::Arc;

/// Cache of native contract method metadata, mirroring the C# NativeContractsCache behaviour.
#[derive(Default)]
pub struct NativeContractsCache {
    entries: HashMap<NativeContractCacheKey, NativeContractsCacheEntry>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct NativeContractCacheKey {
    id: i32,
    hash: UInt160,
}

impl NativeContractCacheKey {
    fn from_contract<P, C>(contract: &C) -> Self
    where
        P: NativeContractProvider + 'static,
        C: NativeContract<P>,
    {
        Self {
            id: contract.id(),
            hash: contract.hash(),
        }
    }
}

impl NativeContractsCache {
    /// Gets the cached entry for the given native contract, building it on demand.
    pub fn get_or_build<'a, P, C>(&'a mut self, contract: &C) -> &'a NativeContractsCacheEntry
    where
        P: NativeContractProvider + 'static,
        C: NativeContract<P>,
    {
        let key = NativeContractCacheKey::from_contract::<P, C>(contract);
        self.entries
            .entry(key)
            .or_insert_with(|| NativeContractsCacheEntry::from_contract::<P, C>(contract))
    }
}

/// Cached metadata for a single native contract.
pub struct NativeContractsCacheEntry {
    methods_by_name: HashMap<String, Vec<CachedNativeMethod>>,
}

struct CachedNativeMethod {
    method_index: usize,
    method: Arc<NativeMethod>,
}

/// Native method metadata selected by the engine for an invocation.
///
/// `method_index` is the index in `NativeContract::methods()`. For standard
/// native contracts that method slice is cloned from the concrete binding table
/// in the same order, so the index can be reused to call the already-resolved
/// Rust handler without repeating name/arity/hardfork selection. The method
/// metadata is shared from the cache, so resolving a native call only clones one
/// `Arc` handle instead of deep-cloning the ABI descriptor.
#[derive(Clone)]
pub struct ResolvedNativeMethod {
    method_index: usize,
    method: Arc<NativeMethod>,
}

impl ResolvedNativeMethod {
    /// Index of the selected method in `NativeContract::methods()`.
    pub fn method_index(&self) -> usize {
        self.method_index
    }

    /// ABI metadata selected for this invocation.
    pub fn method(&self) -> &NativeMethod {
        self.method.as_ref()
    }
}

impl NativeContractsCacheEntry {
    fn from_contract<P, C>(contract: &C) -> Self
    where
        P: NativeContractProvider + 'static,
        C: NativeContract<P>,
    {
        let mut methods_by_name: HashMap<String, Vec<CachedNativeMethod>> = HashMap::new();
        for (method_index, method) in contract.methods().iter().enumerate() {
            let method = Arc::new(method.clone());
            methods_by_name
                .entry(method.name.clone())
                .or_default()
                .push(CachedNativeMethod {
                    method_index,
                    method,
                });
        }

        Self { methods_by_name }
    }

    /// Gets the method metadata entry matching `name`, `parameter_count`, and activation state.
    pub fn get_method(
        &self,
        name: &str,
        parameter_count: usize,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> CoreResult<Option<ResolvedNativeMethod>> {
        let Some(candidates) = self.methods_by_name.get(name) else {
            return Ok(None);
        };

        let mut active = candidates.iter().filter(|entry| {
            entry.method.parameters.len() == parameter_count
                && entry.method.is_active(settings, block_height)
        });

        let Some(selected) = active.next() else {
            return Ok(None);
        };

        if active.next().is_some() {
            return Err(CoreError::invalid_operation(format!(
                "Ambiguous native method '{}({})' at height {}",
                name, parameter_count, block_height
            )));
        }

        Ok(Some(ResolvedNativeMethod {
            method_index: selected.method_index,
            method: selected.method.clone(),
        }))
    }
}

#[cfg(test)]
#[path = "../tests/native/native_contract_cache.rs"]
mod tests;
