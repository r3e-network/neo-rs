use super::OracleHttpsProtocol;
use super::security::{validate_url_for_ssrf, validated_external_addresses};
use crate::network::p2p::payloads::OracleResponseCode;
use crate::oracle_service::settings::MAX_ORACLE_RESPONSE_SIZE;
use futures::StreamExt;

use super::super::OracleServiceSettings;

/// Maximum time to wait for response headers.
#[allow(dead_code)]
const HEADER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Maximum number of redirects to follow.
const MAX_REDIRECTS: u8 = 2;

impl OracleHttpsProtocol {
    pub(crate) async fn process(
        &self,
        settings: &OracleServiceSettings,
        mut uri: url::Url,
    ) -> (OracleResponseCode, String) {
        // Validate URL against SSRF patterns
        if let Err(reason) = validate_url_for_ssrf(uri.as_str()) {
            tracing::warn!(
                target: "neo::oracle",
                url = %uri,
                reason = %reason,
                "SSRF validation failed"
            );
            return (OracleResponseCode::Forbidden, String::new());
        }

        // Check URL against whitelist/blacklist
        if !settings.is_url_allowed(uri.as_str()) {
            tracing::warn!(
                target: "neo::oracle",
                url = %uri,
                "URL blocked by whitelist/blacklist"
            );
            return (OracleResponseCode::Forbidden, String::new());
        }

        let mut redirects = MAX_REDIRECTS;
        loop {
            // R15: resolve once per hop, validate EVERY address, and pin the
            // connection to those addresses so the address that was checked
            // and the address actually connected to cannot diverge (DNS
            // rebind or multi-address fallback). `None` selects the shared
            // unpinned client, which is only acceptable when the operator
            // explicitly allows private hosts.
            let pinned_client = if settings.allow_private_host {
                None
            } else {
                match validated_external_addresses(&uri).await {
                    Ok(Some(addrs)) if addrs.is_empty() => None,
                    Ok(Some(addrs)) => {
                        let host = uri.host_str().unwrap_or_default().to_string();
                        let mut builder = Self::base_client_builder();
                        for addr in &addrs {
                            // Port 0 keeps the URL's port at connect time.
                            builder = builder.resolve(host.as_str(), *addr);
                        }
                        Some(match builder.build() {
                            Ok(client) => client,
                            Err(e) => {
                                tracing::warn!(
                                    target: "neo::oracle",
                                    url = %uri,
                                    error = %e,
                                    "Failed to build address-pinned HTTP client"
                                );
                                return (OracleResponseCode::Error, String::new());
                            }
                        })
                    }
                    Ok(None) => {
                        tracing::warn!(
                            target: "neo::oracle",
                            url = %uri,
                            "Blocked request to internal host"
                        );
                        return (OracleResponseCode::Forbidden, String::new());
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "neo::oracle",
                            url = %uri,
                            error = %e,
                            "DNS lookup failed"
                        );
                        return (OracleResponseCode::Timeout, String::new());
                    }
                }
            };
            let client = pinned_client.as_ref().unwrap_or_else(|| self.client());

            let request = client
                .get(uri.clone())
                .timeout(settings.https_timeout)
                .header(
                    reqwest::header::ACCEPT,
                    settings.allowed_content_types.join(", "),
                );

            let response: reqwest::Response = match request.send().await {
                Ok(response) => response,
                Err(e) => {
                    tracing::warn!(
                        target: "neo::oracle",
                        url = %uri,
                        error = %e,
                        "HTTP request failed"
                    );
                    return if e.is_timeout() {
                        (OracleResponseCode::Timeout, String::new())
                    } else {
                        (OracleResponseCode::Error, format!("Request failed: {}", e))
                    };
                }
            };

            // Handle redirects
            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if let Ok(location) = location.to_str() {
                    if let Ok(next_uri) = url::Url::parse(location) {
                        // R15: never follow a scheme-downgrading redirect; an
                        // HTTPS origin could otherwise steer this node to a
                        // plain-HTTP target whose TLS identity was never
                        // validated.
                        if uri.scheme() == "https" && next_uri.scheme() != "https" {
                            tracing::warn!(
                                target: "neo::oracle",
                                url = %next_uri,
                                "Redirect downgrades from HTTPS; refusing"
                            );
                            return (OracleResponseCode::Forbidden, String::new());
                        }

                        // Validate redirect URL
                        if let Err(reason) = validate_url_for_ssrf(next_uri.as_str()) {
                            tracing::warn!(
                                target: "neo::oracle",
                                url = %next_uri,
                                reason = %reason,
                                "Redirect URL SSRF validation failed"
                            );
                            return (OracleResponseCode::Forbidden, String::new());
                        }

                        // Check redirect URL against whitelist/blacklist
                        if !settings.is_url_allowed(next_uri.as_str()) {
                            tracing::warn!(
                                target: "neo::oracle",
                                url = %next_uri,
                                "Redirect URL blocked by whitelist/blacklist"
                            );
                            return (OracleResponseCode::Forbidden, String::new());
                        }

                        uri = next_uri;
                        if redirects > 0 {
                            redirects -= 1;
                            continue;
                        }
                        return (OracleResponseCode::Timeout, String::new());
                    }
                }
                return (OracleResponseCode::Timeout, String::new());
            }

            // Check HTTP status codes
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return (OracleResponseCode::NotFound, String::new());
            }
            if response.status() == reqwest::StatusCode::FORBIDDEN {
                return (OracleResponseCode::Forbidden, String::new());
            }
            if !response.status().is_success() {
                return (OracleResponseCode::Error, response.status().to_string());
            }

            // Validate content type
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next());
            let Some(content_type) = content_type else {
                return (OracleResponseCode::Error, String::new());
            };
            if !settings.is_content_type_allowed(content_type) {
                tracing::warn!(
                    target: "neo::oracle",
                    url = %uri,
                    content_type = %content_type,
                    "Content type not allowed"
                );
                return (OracleResponseCode::ContentTypeNotSupported, String::new());
            }

            // Check content length header if present
            if let Some(len) = response.content_length() {
                if len as usize > settings.max_response_size {
                    tracing::warn!(
                        target: "neo::oracle",
                        url = %uri,
                        size = len,
                        max_size = settings.max_response_size,
                        "Response too large (Content-Length)"
                    );
                    return (OracleResponseCode::ResponseTooLarge, String::new());
                }
            }

            // Validate charset (only UTF-8 allowed)
            let charset = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| {
                    value
                        .split(';')
                        .map(|part| part.trim())
                        .find_map(|part| part.strip_prefix("charset="))
                        .map(|value| value.trim().to_ascii_lowercase())
                });
            if let Some(charset) = charset {
                if charset != "utf-8" && charset != "utf8" {
                    tracing::warn!(
                        target: "neo::oracle",
                        url = %uri,
                        charset = %charset,
                        "Non-UTF-8 charset not supported"
                    );
                    return (OracleResponseCode::Error, String::new());
                }
            }

            // Read response body with size limit
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        tracing::warn!(
                            target: "neo::oracle",
                            url = %uri,
                            error = %e,
                            "Failed to read response chunk"
                        );
                        return (OracleResponseCode::Error, String::new());
                    }
                };

                // R18: reqwest's byte stream makes no per-chunk size promise,
                // so a single large chunk is not a policy violation — the
                // cumulative limit below is the authoritative constraint and
                // the result must not depend on how the body was chunked.
                if body.len() + chunk.len() > settings.max_response_size {
                    tracing::warn!(
                        target: "neo::oracle",
                        url = %uri,
                        size = body.len() + chunk.len(),
                        max_size = settings.max_response_size,
                        "Response too large"
                    );
                    return (OracleResponseCode::ResponseTooLarge, String::new());
                }
                body.extend_from_slice(&chunk);
            }

            // Validate final body size
            if body.len() > MAX_ORACLE_RESPONSE_SIZE {
                tracing::warn!(
                    target: "neo::oracle",
                    url = %uri,
                    size = body.len(),
                    max_size = MAX_ORACLE_RESPONSE_SIZE,
                    "Response exceeds maximum allowed size"
                );
                return (OracleResponseCode::ResponseTooLarge, String::new());
            }

            let text = match String::from_utf8(body) {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!(
                        target: "neo::oracle",
                        url = %uri,
                        error = %e,
                        "Response is not valid UTF-8"
                    );
                    return (OracleResponseCode::Error, String::new());
                }
            };

            return (OracleResponseCode::Success, text);
        }
    }
}
