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

/// Detaches `context` during shutdown without clearing a newer active system.
///
/// The registry owns a strong reference to the context, which in turn owns the
/// primary store. Failing to detach it keeps persistent backends locked after
/// the actor hierarchy has stopped.
pub(crate) fn clear_system_context(context: &Arc<dyn SystemContext>) {
    let mut active = SYSTEM_CONTEXT
        .write()
        .expect("system context lock poisoned");
    if active
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, context))
    {
        *active = None;
    }
}
