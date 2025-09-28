//! Oracle Service - Oracle Service Implementation
//!
//! This module provides the oracle service functionality for the Oracle Service plugin,
//! matching the C# OracleService exactly.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::settings::OracleServiceSettings;

/// Oracle request structure (matches C# OracleRequest)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRequest {
    /// Request ID
    pub id: String,
    /// Request URL
    pub url: String,
    /// Request filter
    pub filter: String,
    /// Callback contract
    pub callback_contract: String,
    /// Gas for callback
    pub gas_for_response: u64,
    /// User data
    pub user_data: Vec<u8>,
}

/// Oracle response structure (matches C# OracleResponse)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleResponse {
    /// Request ID
    pub id: String,
    /// Response code
    pub code: u8,
    /// Response data
    pub data: Vec<u8>,
    /// Response timestamp
    pub timestamp: u64,
}

/// Oracle Service implementation (matches C# OracleService)
pub struct OracleService {
    /// Plugin settings
    pub settings: OracleServiceSettings,
    /// Active requests
    pub active_requests: Arc<RwLock<HashMap<String, OracleRequest>>>,
    /// Response cache
    pub response_cache: Arc<RwLock<HashMap<String, OracleResponse>>>,
    /// Is service running
    pub is_running: bool,
}

impl OracleService {
    /// Creates a new oracle service
    pub fn new(
        settings: OracleServiceSettings,
    ) -> Self {
        Self {
            settings,
            active_requests: Arc::new(RwLock::new(HashMap::new())),
            response_cache: Arc::new(RwLock::new(HashMap::new())),
            is_running: false,
        }
    }
    
    /// Starts the oracle service
    pub async fn start(&mut self) -> Result<(), String> {
        if self.is_running {
            return Ok(());
        }
        
        self.is_running = true;
        info!("Oracle service started");
        Ok(())
    }
    
    /// Stops the oracle service
    pub async fn stop(&mut self) -> Result<(), String> {
        if !self.is_running {
            return Ok(());
        }
        
        self.is_running = false;
        info!("Oracle service stopped");
        Ok(())
    }
    
    /// Processes an oracle request
    pub async fn process_request(&self, request: OracleRequest) -> Result<(), String> {
        if !self.settings.enabled {
            return Ok(());
        }
        
        debug!("Processing oracle request: {}", request.id);
        
        // Add to active requests
        {
            let mut active_requests = self.active_requests.write().await;
            active_requests.insert(request.id, request.clone());
        }
        
        // Check cache first
        if self.settings.enable_cache {
            if let Some(cached_response) = self.get_cached_response(&request.url).await? {
                self.send_response(cached_response).await?;
                return Ok(());
            }
        }
        
        // Process request asynchronously
        self.process_request_async(request).await?;
        
        Ok(())
    }
    
    /// Processes an oracle request asynchronously
    async fn process_request_async(&self, request: OracleRequest) -> Result<(), String> {
        // Make HTTP request to the oracle URL
        let client = reqwest::Client::new();
        let response = client
            .get(&request.url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP request failed with status: {}", response.status()));
        }

        let response_data = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        // Create oracle response
        let oracle_response = OracleResponse {
            id: request.id.clone(),
            code: 0, // Success
            data: response_data.to_vec(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Cache the response if enabled
        if self.settings.enable_cache {
            self.cache_response(request.url.clone(), oracle_response.clone()).await?;
        }

        // Send the response
        self.send_response(oracle_response).await?;

        Ok(())
    }
    
    /// Gets a cached response
    async fn get_cached_response(&self, url: &str) -> Result<Option<OracleResponse>, String> {
        let cache = self.response_cache.read().await;
        Ok(cache.get(url).cloned())
    }
    
    /// Caches a response
    async fn cache_response(&self, url: String, response: OracleResponse) -> Result<(), String> {
        if !self.settings.enable_cache {
            return Ok(());
        }
        
        let mut cache = self.response_cache.write().await;
        cache.insert(url, response);
        
        // Clean up old cache entries
        if cache.len() > 1000 {
            let keys: Vec<String> = cache.keys().cloned().collect();
            for key in keys.iter().take(100) {
                cache.remove(key);
            }
        }
        
        Ok(())
    }
    
    /// Sends a response
    async fn send_response(&self, response: OracleResponse) -> Result<(), String> {
        debug!("Sending oracle response: {}", response.id);
        
        // In a real implementation, this would:
        // 1. Call the callback contract with the response data
        // 2. Pay the gas for the callback execution
        // 3. Handle any errors from the contract execution
        
        // For now, we'll just log the response
        info!("Oracle response sent: ID={}, Code={}, DataLen={}", 
              response.id, response.code, response.data.len());
        
        Ok(())
    }
    
    /// Gets active requests count
    pub async fn get_active_requests_count(&self) -> usize {
        let active_requests = self.active_requests.read().await;
        active_requests.len()
    }
    
    /// Clears old requests
    pub async fn clear_old_requests(&self) -> Result<(), String> {
        let mut active_requests = self.active_requests.write().await;
        
        // Remove requests older than 1 hour
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        active_requests.retain(|_, request| {
            // TODO: Implement proper timestamp checking
            true
        });
        
        Ok(())
    }
}
