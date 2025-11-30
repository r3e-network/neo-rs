// Copyright (C) 2015-2025 The Neo Project.
//
// i_service_added_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Service added handler interface matching C# IServiceAddedHandler exactly
pub trait IServiceAddedHandler {
    /// The handler of ServiceAdded event from the NeoSystem.
    /// Triggered when a service is added to the NeoSystem.
    /// Matches C# NeoSystem_ServiceAdded_Handler method
    fn neo_system_service_added_handler(
        &self,
        sender: &dyn std::any::Any,
        service: &dyn std::any::Any,
    );
}
