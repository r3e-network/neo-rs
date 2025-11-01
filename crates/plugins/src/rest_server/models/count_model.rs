// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Models.CountModel.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CountModel {
    pub count: i32,
}

impl CountModel {
    pub fn new(count: i32) -> Self {
        Self { count }
    }
}
