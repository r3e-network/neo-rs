//! Process-wide registry for the active [`SystemContext`].
//!
//! Historically this global lived inside `smart_contract::native::helpers::NativeHelpers`,
//! because the in-tree native contracts reached for it directly. Native contracts are now
//! owned by the `neo-native-contracts` crate, which receives chain access through the
//! `NativeChainQueries` parameter object instead of a global. The `SystemContext` trait is
//! still defined by neo-core, so the registry stays here as a node-lifecycle concern.

use std::sync::{Arc, LazyLock, RwLock};

use crate::services::SystemContext;

static SYSTEM_CONTEXT: LazyLock<RwLock<Option<Arc<dyn SystemContext>>>> =
    LazyLock::new(|| RwLock::new(None));

/// Attaches the system context used by runtime subsystems.
pub(crate) fn attach_system_context(context: Arc<dyn SystemContext>) {
    *SYSTEM_CONTEXT
        .write()
        .expect("system context lock poisoned") = Some(context);
}

/// Returns the attached system context, if one has been set.
#[allow(dead_code)]
pub(crate) fn context() -> Option<Arc<dyn SystemContext>> {
    SYSTEM_CONTEXT
        .read()
        .expect("system context lock poisoned")
        .clone()
}

/// Clears the attached system context; used by tests to reset global state.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn clear_system_context() {
    *SYSTEM_CONTEXT
        .write()
        .expect("system context lock poisoned") = None;
}
