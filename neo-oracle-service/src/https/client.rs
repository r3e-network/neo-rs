//! Oracle HTTPS client construction.

use crate::service::OracleServiceError;

#[derive(Clone)]
pub(crate) struct OracleHttpsProtocol {
    client: reqwest::Client,
}

impl OracleHttpsProtocol {
    pub(crate) fn new() -> Result<Self, OracleServiceError> {
        let version = env!("CARGO_PKG_VERSION");
        Self::from_builder(
            reqwest::Client::builder().user_agent(format!("NeoOracleService/{}", version)),
        )
    }

    fn from_builder(builder: reqwest::ClientBuilder) -> Result<Self, OracleServiceError> {
        let client = builder
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| OracleServiceError::HttpClientInitialization(err.to_string()))?;
        Ok(Self { client })
    }

    pub(crate) fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

#[cfg(test)]
#[path = "../tests/https/client.rs"]
mod tests;
