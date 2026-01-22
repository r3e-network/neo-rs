use super::super::OracleService;
use crate::wallets::KeyPair;
use tokio::task::JoinHandle;

#[cfg(feature = "oracle")]
use super::super::SIGNATURE_SEND_TIMEOUT;
#[cfg(feature = "oracle")]
use base64::Engine as _;
#[cfg(feature = "oracle")]
use std::sync::atomic::Ordering;
#[cfg(feature = "oracle")]
use tracing::warn;

impl OracleService {
    #[cfg(feature = "oracle")]
    pub(in super::super) fn send_response_signature(
        &self,
        request_id: u64,
        tx_sign: Vec<u8>,
        key: KeyPair,
    ) -> JoinHandle<()> {
        let settings = self.settings.clone();
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let https = self.https.clone();
        tokio::spawn(async move {
            let Ok(public_key) = key.get_public_key_point() else {
                return;
            };
            let mut message = Vec::with_capacity(public_key.as_bytes().len() + 8 + tx_sign.len());
            message.extend_from_slice(public_key.as_bytes());
            message.extend_from_slice(&request_id.to_le_bytes());
            message.extend_from_slice(&tx_sign);

            let sign = match key.sign(&message) {
                Ok(sign) => sign,
                Err(_) => return,
            };

            let payload = serde_json::json!({
                "id": counter,
                "jsonrpc": "2.0",
                "method": "submitoracleresponse",
                "params": [
                    base64::engine::general_purpose::STANDARD.encode(public_key.as_bytes()),
                    request_id,
                    base64::engine::general_purpose::STANDARD.encode(&tx_sign),
                    base64::engine::general_purpose::STANDARD.encode(&sign)
                ]
            });

            for node in settings.nodes.iter() {
                let url = match url::Url::parse(node) {
                    Ok(url) => url,
                    Err(_) => {
                        warn!(target: "neo::oracle", node = %node, "invalid oracle node endpoint");
                        continue;
                    }
                };
                let res = https
                    .client()
                    .post(url)
                    .timeout(SIGNATURE_SEND_TIMEOUT)
                    .json(&payload)
                    .send()
                    .await;
                match res {
                    Ok(response) => {
                        if !response.status().is_success() {
                            warn!(
                                target: "neo::oracle",
                                node = %node,
                                status = %response.status(),
                                "oracle signature rejected"
                            );
                        }
                    }
                    Err(err) => {
                        warn!(target: "neo::oracle", node = %node, %err, "failed to send oracle signature");
                    }
                }
            }
        })
    }

    #[cfg(not(feature = "oracle"))]
    pub(in super::super) fn send_response_signature(
        &self,
        request_id: u64,
        tx_sign: Vec<u8>,
        key: KeyPair,
    ) -> JoinHandle<()> {
        let _ = (&self.settings, request_id, tx_sign, key);
        tokio::spawn(async move {})
    }
}
