//! ContractManagement protocol constants and event names.
//!
//! Centralizes native event names, storage prefixes, and genesis defaults so
//! the contract root stays focused on the native-contract surface.

pub(crate) const CONTRACT_DEPLOY_EVENT: &str = "Deploy";
pub(crate) const CONTRACT_UPDATE_EVENT: &str = "Update";
pub(crate) const CONTRACT_DESTROY_EVENT: &str = "Destroy";

/// Storage prefix for the minimum-deployment-fee setting (C#
/// `ContractManagement.Prefix_MinimumDeploymentFee`).
pub(in crate::contract_management) const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
/// C# default minimum deployment fee: 10 GAS, in datoshi.
pub(in crate::contract_management) const DEFAULT_MINIMUM_DEPLOYMENT_FEE: i64 = 10_00000000;

/// Storage prefix for the per-contract record (matches C#
/// `ContractManagement.PREFIX_CONTRACT`).
pub(in crate::contract_management) const PREFIX_CONTRACT: u8 = 8;
/// Storage prefix for the contract-id -> hash index (matches C#
/// `ContractManagement.PREFIX_CONTRACT_HASH`).
pub(in crate::contract_management) const PREFIX_CONTRACT_HASH: u8 = 12;
/// Storage prefix for the next-available-contract-id counter (matches C#
/// `ContractManagement.Prefix_NextAvailableId`).
pub(in crate::contract_management) const PREFIX_NEXT_AVAILABLE_ID: u8 = 15;
/// C# genesis value for `Prefix_NextAvailableId` (`InitializeAsync` writes 1).
pub(in crate::contract_management) const DEFAULT_NEXT_AVAILABLE_ID: i64 = 1;
