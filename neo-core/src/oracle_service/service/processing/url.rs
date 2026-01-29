use super::super::OracleService;
use crate::network::p2p::payloads::OracleResponseCode;
use crate::wallets::KeyPair;

/// Maximum URL length allowed.
const MAX_URL_LENGTH: usize = 2048;

/// Maximum number of redirects allowed.
const MAX_REDIRECTS: u8 = 2;

impl OracleService {
    pub(in super::super) async fn process_url(
        &self,
        url: &str,
        oracle_key: Option<&KeyPair>,
    ) -> (OracleResponseCode, String) {
        // Validate URL length
        if url.len() > MAX_URL_LENGTH {
            tracing::warn!(
                target: "neo::oracle",
                url_length = url.len(),
                max_length = MAX_URL_LENGTH,
                "URL too long"
            );
            return (
                OracleResponseCode::Error,
                format!("URL too long: {} > {}", url.len(), MAX_URL_LENGTH),
            );
        }

        // Validate URL is not empty
        if url.is_empty() {
            return (OracleResponseCode::Error, "Empty URL".to_string());
        }

        // Handle NeoFS protocol
        if url.len() >= 6 && url[..6].eq_ignore_ascii_case("neofs:") {
            #[cfg(feature = "oracle")]
            {
                return self.neofs.process(&self.settings, url, oracle_key).await;
            }
            #[cfg(not(feature = "oracle"))]
            {
                return (
                    OracleResponseCode::ProtocolNotSupported,
                    format!("Invalid Protocol:<{url}>"),
                );
            }
        }

        // Parse and validate URL
        let parsed = match url::Url::parse(url) {
            Ok(uri) => uri,
            Err(e) => {
                tracing::warn!(
                    target: "neo::oracle",
                    url = %url,
                    error = %e,
                    "Failed to parse URL"
                );
                return (OracleResponseCode::Error, format!("Invalid url:<{url}>"));
            }
        };

        // Validate scheme
        let scheme = parsed.scheme();
        if scheme.eq_ignore_ascii_case("https") {
            #[cfg(feature = "oracle")]
            {
                // Apply timeout with a maximum ceiling
                let timeout = std::cmp::min(
                    self.settings.max_oracle_timeout,
                    std::time::Duration::from_secs(60),
                );
                let fut = self.https.process(&self.settings, parsed);
                return match tokio::time::timeout(timeout, fut).await {
                    Ok(result) => result,
                    Err(_) => {
                        tracing::warn!(
                            target: "neo::oracle",
                            url = %url,
                            timeout_secs = timeout.as_secs(),
                            "Oracle request timed out"
                        );
                        (OracleResponseCode::Timeout, String::new())
                    }
                };
            }
        }

        if scheme.eq_ignore_ascii_case("neofs") {
            #[cfg(feature = "oracle")]
            {
                return self.neofs.process(&self.settings, url, oracle_key).await;
            }
            #[cfg(not(feature = "oracle"))]
            {
                return (
                    OracleResponseCode::ProtocolNotSupported,
                    format!("Invalid Protocol:<{url}>"),
                );
            }
        }

        (
            OracleResponseCode::ProtocolNotSupported,
            format!("Invalid Protocol:<{url}>"),
        )
    }
}
