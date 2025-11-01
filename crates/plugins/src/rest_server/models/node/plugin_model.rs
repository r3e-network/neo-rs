// Copyright (C) 2015-2025 The Neo Project.
//
// plugin_model.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Plugin model matching C# PluginModel exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginModel {
    /// Name
    /// Matches C# Name property
    pub name: String,

    /// Version
    /// Matches C# Version property
    pub version: String,

    /// Description
    /// Matches C# Description property
    pub description: String,
}

impl PluginModel {
    /// Creates a new PluginModel
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self {
            name: String::new(),
            version: String::new(),
            description: String::new(),
        }
    }

    /// Creates a new PluginModel with parameters
    /// Matches C# constructor with parameters
    pub fn with_params(name: String, version: String, description: String) -> Self {
        Self {
            name,
            version,
            description,
        }
    }
}

impl Default for PluginModel {
    fn default() -> Self {
        Self::new()
    }
}
