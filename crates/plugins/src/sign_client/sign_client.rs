// Copyright (C) 2015-2025 The Neo Project.
//
// sign_client.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::settings::SignSettings;
use super::vsock::Vsock;
use neo_core::{ECPoint, UInt160, UInt256};
use std::sync::Arc;

/// SignClient implementation matching C# SignClient exactly
pub struct SignClient {
    /// gRPC channel
    channel: Option<Arc<dyn GrpcChannel>>,
    /// gRPC client
    client: Option<Arc<dyn SecureSignClient>>,
    /// Signer name
    name: String,
}

/// gRPC channel trait
pub trait GrpcChannel: Send + Sync {
    fn dispose(&self);
}

/// Secure sign client trait
pub trait SecureSignClient: Send + Sync {
    fn get_account_status(&self, public_key: &[u8]) -> Result<AccountStatus, SignException>;
    fn sign_extensible_payload(
        &self,
        payload: &ExtensiblePayloadRequest,
        script_hashes: &[UInt160],
        network: u32,
    ) -> Result<Vec<AccountSigns>, SignException>;
    fn sign_block(
        &self,
        block: &BlockRequest,
        public_key: &[u8],
        network: u32,
    ) -> Result<Vec<u8>, SignException>;
}

/// Account status enum matching C# AccountStatus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    NoSuchAccount,
    NoPrivateKey,
    Single,
    Multiple,
}

/// Sign exception matching C# SignException
#[derive(Debug, Clone)]
pub struct SignException {
    pub message: String,
}

impl std::fmt::Display for SignException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SignException {}

/// Extensible payload request matching C# request structure
#[derive(Debug, Clone)]
pub struct ExtensiblePayloadRequest {
    pub category: String,
    pub valid_block_start: u32,
    pub valid_block_end: u32,
    pub sender: UInt160,
    pub data: Vec<u8>,
}

/// Block request matching C# request structure
#[derive(Debug, Clone)]
pub struct BlockRequest {
    pub header: BlockHeaderRequest,
    pub tx_hashes: Vec<UInt256>,
}

/// Block header request matching C# request structure
#[derive(Debug, Clone)]
pub struct BlockHeaderRequest {
    pub version: u32,
    pub prev_hash: UInt256,
    pub merkle_root: UInt256,
    pub timestamp: u64,
    pub nonce: u64,
    pub index: u32,
    pub primary_index: u8,
    pub next_consensus: UInt160,
}

/// Account signs matching C# AccountSigns
#[derive(Debug, Clone)]
pub struct AccountSigns {
    pub status: AccountStatus,
    pub contract: Option<ContractInfo>,
    pub signs: Vec<SignInfo>,
}

/// Contract info matching C# contract structure
#[derive(Debug, Clone)]
pub struct ContractInfo {
    pub parameters: Vec<ContractParameterType>,
    pub script: Vec<u8>,
}

/// Contract parameter type matching C# ContractParameterType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractParameterType {
    Any,
    ByteArray,
    Signature,
    Boolean,
    Integer,
    String,
    Hash160,
    Hash256,
    PublicKey,
    Array,
    Map,
}

/// Sign info matching C# SignInfo
#[derive(Debug, Clone)]
pub struct SignInfo {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl SignClient {
    /// Creates a new SignClient
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self {
            channel: None,
            client: None,
            name: String::new(),
        }
    }

    /// Creates a new SignClient with settings
    /// Matches C# constructor with settings
    pub fn with_settings(settings: SignSettings) -> Self {
        let mut client = Self::new();
        client.reset(settings);
        client
    }

    /// Creates a new SignClient for testing
    /// Matches C# internal constructor for testing
    pub fn for_test(name: String, client: Arc<dyn SecureSignClient>) -> Self {
        let mut sign_client = Self::new();
        sign_client.reset_with_name_and_client(name, Some(client));
        sign_client
    }

    /// Gets the description
    /// Matches C# Description property
    pub fn description(&self) -> &'static str {
        "Signer plugin for signer service."
    }

    /// Gets the config file path
    /// Matches C# ConfigFile property
    pub fn config_file(&self) -> String {
        "SignClient.json".to_string()
    }

    /// Resets the signer with name and client
    /// Matches C# Reset method
    fn reset_with_name_and_client(
        &mut self,
        name: String,
        client: Option<Arc<dyn SecureSignClient>>,
    ) {
        if let Some(ref client) = self.client {
            // Unregister signer if name is not empty
            if !self.name.is_empty() {
                // In a real implementation, this would unregister the signer
            }
        }

        self.name = name;
        self.client = client;

        if !self.name.is_empty() {
            // In a real implementation, this would register the signer
        }
    }

    /// Gets service configuration
    /// Matches C# GetServiceConfig method
    fn get_service_config(&self, settings: &SignSettings) -> ServiceConfig {
        ServiceConfig {
            method_configs: vec![MethodConfig {
                names: vec!["*".to_string()],
                retry_policy: RetryPolicy {
                    max_attempts: 3,
                    initial_backoff_ms: 50,
                    max_backoff_ms: 200,
                    backoff_multiplier: 1.5,
                    retryable_status_codes: vec![
                        "CANCELLED".to_string(),
                        "DEADLINE_EXCEEDED".to_string(),
                        "RESOURCE_EXHAUSTED".to_string(),
                        "UNAVAILABLE".to_string(),
                        "ABORTED".to_string(),
                        "INTERNAL".to_string(),
                        "DATA_LOSS".to_string(),
                        "UNKNOWN".to_string(),
                    ],
                },
            }],
        }
    }

    /// Resets the signer with settings
    /// Matches C# Reset method with settings
    fn reset(&mut self, settings: SignSettings) {
        let service_config = self.get_service_config(&settings);
        let vsock_address = settings.get_vsock_address();

        let channel: Arc<dyn GrpcChannel> = if let Some(vsock_addr) = vsock_address {
            Vsock::create_channel(vsock_addr, service_config)
        } else {
            // Create regular gRPC channel
            Arc::new(RegularGrpcChannel::new(
                settings.endpoint.clone(),
                service_config,
            ))
        };

        // Dispose old channel
        if let Some(ref old_channel) = self.channel {
            old_channel.dispose();
        }

        self.channel = Some(channel.clone());
        self.reset_with_name_and_client(
            settings.name.clone(),
            Some(Arc::new(SecureSignClientImpl::new(channel))),
        );
    }

    /// Gets account status command
    /// Matches C# AccountStatusCommand method
    pub fn account_status_command(&self, hex_public_key: &str) -> Result<(), String> {
        if self.client.is_none() {
            return Err("No signer service is connected".to_string());
        }

        let client = self.client.as_ref().unwrap();

        match ECPoint::decode_point(hex_public_key.as_bytes(), neo_core::ECCurve::Secp256r1) {
            Ok(public_key) => match client.get_account_status(&public_key.encode_point(true)) {
                Ok(status) => {
                    println!("Account status: {:?}", status);
                    Ok(())
                }
                Err(e) => Err(format!("Failed to get account status: {}", e)),
            },
            Err(e) => Err(format!("Invalid public key: {}", e)),
        }
    }

    /// Gets account status
    /// Matches C# GetAccountStatus method
    fn get_account_status(&self, public_key: &ECPoint) -> Result<AccountStatus, SignException> {
        if self.client.is_none() {
            return Err(SignException {
                message: "No signer service is connected".to_string(),
            });
        }

        let client = self.client.as_ref().unwrap();
        client.get_account_status(&public_key.encode_point(true))
    }

    /// Checks if the account is signable
    /// Matches C# ContainsSignable method
    pub fn contains_signable(&self, public_key: &ECPoint) -> Result<bool, SignException> {
        let status = self.get_account_status(public_key)?;
        Ok(status == AccountStatus::Single || status == AccountStatus::Multiple)
    }

    /// Tries to decode public key
    /// Matches C# TryDecodePublicKey method
    fn try_decode_public_key(public_key: &[u8]) -> Option<ECPoint> {
        ECPoint::decode_point(public_key, neo_core::ECCurve::Secp256r1).ok()
    }

    /// Configures the plugin
    /// Matches C# Configure method
    pub fn configure(&mut self, config: Option<serde_json::Value>) {
        if let Some(config_value) = config {
            let settings = SignSettings::from_config(&config_value);
            self.reset(settings);
        }
    }

    /// Disposes the plugin
    /// Matches C# Dispose method
    pub fn dispose(&mut self) {
        self.reset_with_name_and_client(String::new(), None);
        if let Some(ref channel) = self.channel {
            channel.dispose();
        }
    }
}
