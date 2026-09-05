use super::{MAX_CALLBACK_LENGTH, MAX_FILTER_LENGTH, MAX_URL_LENGTH, MAX_USER_DATA_LENGTH};
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::neo_config::MAX_SCRIPT_SIZE;

/// Oracle configuration parameters.
#[derive(Debug, Clone)]
pub struct OracleConfig {
    /// Maximum URL length.
    pub max_url_length: usize,

    /// Maximum filter length.
    pub max_filter_length: usize,

    /// Maximum callback method name length.
    pub max_callback_length: usize,

    /// Maximum user data length.
    pub max_user_data_length: usize,

    /// Maximum response data length.
    pub max_response_length: usize,

    /// Request timeout in blocks.
    pub request_timeout: u32,

    /// Minimum gas for response.
    pub min_response_gas: i64,

    /// Maximum gas for response.
    pub max_response_gas: i64,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            max_url_length: MAX_URL_LENGTH,
            max_filter_length: MAX_FILTER_LENGTH,
            max_callback_length: MAX_CALLBACK_LENGTH,
            max_user_data_length: MAX_USER_DATA_LENGTH,
            max_response_length: MAX_SCRIPT_SIZE,
            request_timeout: 144, // ~24 hours at 10 second blocks
            min_response_gas: 10_000_000,
            max_response_gas: i64::MAX,
        }
    }
}

/// Builder for `OracleConfig` with fluent API and validation.
#[derive(Debug, Clone)]
pub struct OracleConfigBuilder {
    config: OracleConfig,
}

impl OracleConfigBuilder {
    /// Creates a new builder with default values.
    #[inline]
    pub fn new() -> Self {
        Self {
            config: OracleConfig::default(),
        }
    }

    /// Sets the maximum URL length.
    #[inline]
    pub fn max_url_length(mut self, len: usize) -> Self {
        self.config.max_url_length = len;
        self
    }

    /// Sets the maximum filter length.
    #[inline]
    pub fn max_filter_length(mut self, len: usize) -> Self {
        self.config.max_filter_length = len;
        self
    }

    /// Sets the maximum callback method name length.
    #[inline]
    pub fn max_callback_length(mut self, len: usize) -> Self {
        self.config.max_callback_length = len;
        self
    }

    /// Sets the maximum user data length.
    #[inline]
    pub fn max_user_data_length(mut self, len: usize) -> Self {
        self.config.max_user_data_length = len;
        self
    }

    /// Sets the maximum response data length.
    #[inline]
    pub fn max_response_length(mut self, len: usize) -> Self {
        self.config.max_response_length = len;
        self
    }

    /// Sets the request timeout in blocks.
    #[inline]
    pub fn request_timeout(mut self, timeout: u32) -> Self {
        self.config.request_timeout = timeout;
        self
    }

    /// Sets the minimum gas for response.
    #[inline]
    pub fn min_response_gas(mut self, gas: i64) -> Self {
        self.config.min_response_gas = gas;
        self
    }

    /// Sets the maximum gas for response.
    #[inline]
    pub fn max_response_gas(mut self, gas: i64) -> Self {
        self.config.max_response_gas = gas;
        self
    }

    /// Validates and builds the configuration.
    pub fn build(self) -> Result<OracleConfig> {
        // Validate constraints
        if self.config.min_response_gas > self.config.max_response_gas {
            return Err(Error::invalid_operation(
                "min_response_gas cannot exceed max_response_gas".to_string(),
            ));
        }
        if self.config.max_url_length == 0 {
            return Err(Error::invalid_operation(
                "max_url_length must be greater than 0".to_string(),
            ));
        }
        Ok(self.config)
    }

    /// Builds without validation (for internal use).
    #[inline]
    pub fn build_unchecked(self) -> OracleConfig {
        self.config
    }
}

crate::impl_default_via_new!(OracleConfigBuilder);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the Oracle request limits to Neo N3 v3.10.1 `OracleContract`.
    ///
    /// These are consensus-relevant: a node that accepts a request the rest of
    /// the network rejects diverges the Oracle contract's on-chain state.
    #[test]
    fn default_limits_match_csharp_oracle_contract() {
        let c = OracleConfig::default();
        assert_eq!(c.max_url_length, 256, "C# OracleContract.MaxUrlLength");
        assert_eq!(
            c.max_filter_length, 128,
            "C# OracleContract.MaxFilterLength"
        );
        assert_eq!(
            c.max_callback_length, 32,
            "C# OracleContract.MaxCallbackLength"
        );
        assert_eq!(
            c.max_user_data_length, 512,
            "C# OracleContract.MaxUserDataLength"
        );
        // C# `MinGasForResponse = 0_10000000` (0.1 GAS in datoshi).
        assert_eq!(c.min_response_gas, 10_000_000, "C# MinGasForResponse");
    }

    #[test]
    fn user_data_limit_is_not_the_wire_tx_limit() {
        // Regression guard: this was previously wired to
        // `BLOCK_MAX_TX_WIRE_LIMIT` (65_535), 128x more permissive than C#.
        assert_ne!(OracleConfig::default().max_user_data_length, 65_535);
    }
}
