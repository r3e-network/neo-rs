use super::super::OracleService;
use crate::network::p2p::payloads::OracleResponseCode;
use crate::wallets::KeyPair;

impl OracleService {
    pub(in super::super) async fn process_url(
        &self,
        url: &str,
        oracle_key: Option<&KeyPair>,
    ) -> (OracleResponseCode, String) {
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

        let parsed = match url::Url::parse(url) {
            Ok(uri) => uri,
            Err(_) => {
                return (OracleResponseCode::Error, format!("Invalid url:<{url}>"));
            }
        };

        let scheme = parsed.scheme();
        if scheme.eq_ignore_ascii_case("https") {
            #[cfg(feature = "oracle")]
            {
                let fut = self.https.process(&self.settings, parsed);
                return match tokio::time::timeout(self.settings.max_oracle_timeout, fut).await {
                    Ok(result) => result,
                    Err(_) => (OracleResponseCode::Timeout, String::new()),
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
