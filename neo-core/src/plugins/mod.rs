//! Compatibility layer that re-exports the canonical plugin types from
//! `crate::extensions`.

pub use crate::extensions::plugin::{
    plugins_directory, Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
    PluginManager, PluginRegistration, UnhandledExceptionPolicy,
};
