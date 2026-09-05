#[derive(Clone)]
pub(crate) struct OracleHttpsProtocol {
    client: reqwest::Client,
}

impl OracleHttpsProtocol {
    pub(crate) fn new() -> Self {
        let client = Self::base_client_builder()
            .build()
            .expect("failed to build oracle http client");
        Self { client }
    }

    /// Base builder for oracle HTTP clients. Pinned-address variants (see
    /// process.rs R15 handling) must share this configuration.
    pub(crate) fn base_client_builder() -> reqwest::ClientBuilder {
        let version = env!("CARGO_PKG_VERSION");
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("NeoOracleService/{}", version))
    }

    pub(crate) fn client(&self) -> &reqwest::Client {
        &self.client
    }
}
