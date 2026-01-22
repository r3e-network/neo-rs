use super::security::is_internal_host;
use super::OracleHttpsProtocol;
use crate::network::p2p::payloads::oracle_response::MAX_RESULT_SIZE;
use crate::network::p2p::payloads::OracleResponseCode;
use futures::StreamExt;

use super::super::OracleServiceSettings;

impl OracleHttpsProtocol {
    pub(crate) async fn process(
        &self,
        settings: &OracleServiceSettings,
        mut uri: url::Url,
    ) -> (OracleResponseCode, String) {
        let mut redirects = 2;
        loop {
            if !settings.allow_private_host {
                match is_internal_host(&uri).await {
                    Ok(true) => return (OracleResponseCode::Forbidden, String::new()),
                    Ok(false) => {}
                    Err(_) => return (OracleResponseCode::Timeout, String::new()),
                }
            }

            let request = self
                .client()
                .get(uri.clone())
                .timeout(settings.https_timeout)
                .header(
                    reqwest::header::ACCEPT,
                    settings.allowed_content_types.join(", "),
                );

            let response: reqwest::Response = match request.send().await {
                Ok(response) => response,
                Err(_) => return (OracleResponseCode::Timeout, String::new()),
            };

            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if let Ok(location) = location.to_str() {
                    if let Ok(next_uri) = url::Url::parse(location) {
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

            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return (OracleResponseCode::NotFound, String::new());
            }
            if response.status() == reqwest::StatusCode::FORBIDDEN {
                return (OracleResponseCode::Forbidden, String::new());
            }
            if !response.status().is_success() {
                return (OracleResponseCode::Error, response.status().to_string());
            }

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next());
            let Some(content_type) = content_type else {
                return (OracleResponseCode::Error, String::new());
            };
            if !settings.is_content_type_allowed(content_type) {
                return (OracleResponseCode::ContentTypeNotSupported, String::new());
            }

            if let Some(len) = response.content_length() {
                if len as usize > MAX_RESULT_SIZE {
                    return (OracleResponseCode::ResponseTooLarge, String::new());
                }
            }

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
                    return (OracleResponseCode::Error, String::new());
                }
            }

            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(_) => return (OracleResponseCode::Error, String::new()),
                };
                if body.len() + chunk.len() > MAX_RESULT_SIZE {
                    return (OracleResponseCode::ResponseTooLarge, String::new());
                }
                body.extend_from_slice(&chunk);
            }

            let text = match String::from_utf8(body) {
                Ok(text) => text,
                Err(_) => return (OracleResponseCode::Error, String::new()),
            };

            return (OracleResponseCode::Success, text);
        }
    }
}
