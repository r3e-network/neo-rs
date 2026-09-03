// Copyright (C) 2015-2025 The Neo Project.
//
// assembly_info.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Assembly information for the Neo project.
/// Matches C# AssemblyInfo.cs exactly
/// 
/// In Rust, we don't have assembly attributes like C#, but we can provide
/// equivalent functionality through module-level constants and functions.
/// 
/// The C# version contains:
/// - InternalsVisibleTo attributes for test assemblies
/// - Assembly metadata

/// Assembly version information
pub const ASSEMBLY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Assembly title
pub const ASSEMBLY_TITLE: &str = "Neo";

/// Assembly description
pub const ASSEMBLY_DESCRIPTION: &str = "Neo blockchain implementation in Rust";

/// Assembly company
pub const ASSEMBLY_COMPANY: &str = "The Neo Project";

/// Assembly product
pub const ASSEMBLY_PRODUCT: &str = "Neo";

/// Assembly copyright
pub const ASSEMBLY_COPYRIGHT: &str = "Copyright (C) 2015-2025 The Neo Project";

/// Assembly trademark
pub const ASSEMBLY_TRADEMARK: &str = "";

/// Assembly culture
pub const ASSEMBLY_CULTURE: &str = "";

/// Assembly configuration
pub const ASSEMBLY_CONFIGURATION: &str = "";

/// Assembly file version
pub const ASSEMBLY_FILE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Assembly informational version
pub const ASSEMBLY_INFORMATIONAL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Assembly GUID
pub const ASSEMBLY_GUID: &str = "00000000-0000-0000-0000-000000000000";

/// Assembly metadata
pub struct AssemblyInfo;

impl AssemblyInfo {
    /// Gets the assembly version
    /// Matches C# Assembly.GetName().Version functionality
    pub fn get_version() -> String {
        ASSEMBLY_VERSION.to_string()
    }
    
    /// Gets the assembly title
    pub fn get_title() -> String {
        ASSEMBLY_TITLE.to_string()
    }
    
    /// Gets the assembly description
    pub fn get_description() -> String {
        ASSEMBLY_DESCRIPTION.to_string()
    }
    
    /// Gets the assembly company
    pub fn get_company() -> String {
        ASSEMBLY_COMPANY.to_string()
    }
    
    /// Gets the assembly product
    pub fn get_product() -> String {
        ASSEMBLY_PRODUCT.to_string()
    }
    
    /// Gets the assembly copyright
    pub fn get_copyright() -> String {
        ASSEMBLY_COPYRIGHT.to_string()
    }
    
    /// Gets the assembly file version
    pub fn get_file_version() -> String {
        ASSEMBLY_FILE_VERSION.to_string()
    }
    
    /// Gets the assembly informational version
    pub fn get_informational_version() -> String {
        ASSEMBLY_INFORMATIONAL_VERSION.to_string()
    }
    
    /// Gets the assembly GUID
    pub fn get_guid() -> String {
        ASSEMBLY_GUID.to_string()
    }
}

/// In C#, the InternalsVisibleTo attributes would be:
/// [assembly: InternalsVisibleTo("DynamicProxyGenAssembly2")]
/// [assembly: InternalsVisibleTo("neo.UnitTests")]
/// [assembly: InternalsVisibleTo("neodebug-3-adapter")]
/// 
/// In Rust, we don't have equivalent assembly attributes, but we can
/// provide similar functionality through module visibility and testing
/// configuration.