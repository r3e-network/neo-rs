#[derive(Clone)]
pub(crate) struct OracleHttpsProtocol {
    client: reqwest::Client,
}

impl OracleHttpsProtocol {
    pub(crate) fn new() -> Self {
        let version = env!("CARGO_PKG_VERSION");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("NeoOracleService/{}", version))
            .build()
            .expect("failed to build oracle http client");
        Self { client }
    }

    pub(crate) fn client(&self) -> &reqwest::Client {
        &self.client
    }
}
