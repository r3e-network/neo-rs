use super::*;
/// HTTPS transport isolated from wallet and node RPC clients.
pub struct MtlsRemoteSignerTransport {
    pub(super) client: Client,
    pub(super) endpoint: Url,
}

impl MtlsRemoteSignerTransport {
    pub fn from_config(provider: &SignerProviderSection) -> Result<Self> {
        let remote = provider
            .remote
            .as_ref()
            .context("validated remote signer has no mTLS configuration")?;
        let endpoint = Url::parse(
            provider
                .endpoint
                .as_deref()
                .context("validated remote signer has no endpoint")?,
        )?;
        let ca_pem = fs::read(&remote.ca_certificate_path)
            .with_context(|| format!("failed to read remote signer CA for '{}'", provider.id))?;
        let client_cert = fs::read(&remote.client_certificate_path).with_context(|| {
            format!(
                "failed to read remote signer client certificate for '{}'",
                provider.id
            )
        })?;
        let client_key = fs::read(&remote.client_private_key_path).with_context(|| {
            format!(
                "failed to read remote signer client key for '{}'",
                provider.id
            )
        })?;
        let ca = Certificate::from_pem(&ca_pem)
            .context("remote signer CA is not a valid PEM certificate")?;
        let mut identity_pem = client_cert;
        identity_pem.extend_from_slice(b"\n");
        identity_pem.extend_from_slice(&client_key);
        let identity = Identity::from_pem(&identity_pem)
            .context("remote signer client certificate/key is not valid PEM identity material")?;
        let client = Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .tls_built_in_root_certs(false)
            .add_root_certificate(ca)
            .identity(identity)
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("failed to build dedicated remote signer mTLS client")?;
        Ok(Self { client, endpoint })
    }

    fn url(&self, path: &str) -> SignResult<Url> {
        self.endpoint
            .join(path)
            .map_err(|error| provider_error(format!("invalid remote signer route: {error}")))
    }

    async fn post<T: serde::Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> SignResult<R> {
        let response = self
            .client
            .post(self.url(path)?)
            .json(body)
            .send()
            .await
            .map_err(|error| provider_error(format!("remote signer transport failed: {error}")))?;
        if response.status() != StatusCode::OK {
            return Err(provider_error(format!(
                "remote signer returned HTTP {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|error| provider_error(format!("invalid remote signer response: {error}")))
    }

    pub(super) fn result_unknown(message: impl Into<String>) -> SignError {
        SignError::new(SignErrorCode::ResultUnknown, message).retryable(true)
    }

    pub(super) fn decode_sign_response(
        status: StatusCode,
        body: &[u8],
    ) -> SignResult<RemoteSignDispatchResult> {
        if status.is_success() {
            return serde_json::from_slice(body)
                .map(RemoteSignDispatchResult::Succeeded)
                .map_err(|error| {
                    Self::result_unknown(format!(
                        "remote signer success response could not be decoded: {error}"
                    ))
                });
        }
        if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            return Err(Self::result_unknown(format!(
                "remote signer returned ambiguous HTTP {status} after dispatch"
            )));
        }
        if status.is_client_error() {
            return serde_json::from_slice::<RemoteSignRejection>(body)
                .map(RemoteSignDispatchResult::Rejected)
                .map_err(|error| {
                    Self::result_unknown(format!(
                        "remote signer returned HTTP {status} without a valid permanent rejection: {error}"
                    ))
                });
        }
        Err(Self::result_unknown(format!(
            "remote signer returned ambiguous HTTP {status} after dispatch"
        )))
    }

    async fn post_sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
        let response = self
            .client
            .post(self.url(SIGN_PATH)?)
            .json(request)
            .send()
            .await
            .map_err(|error| {
                Self::result_unknown(format!(
                    "remote signer connection failed after dispatch became possible: {error}"
                ))
            })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|error| {
            Self::result_unknown(format!(
                "remote signer response body was interrupted after dispatch: {error}"
            ))
        })?;
        Self::decode_sign_response(status, &body)
    }
}

#[async_trait]
impl RemoteSignerTransport for MtlsRemoteSignerTransport {
    async fn key_info(&self, key_ref: &KeyRef) -> SignResult<RemoteKeyInfoResponse> {
        self.post(KEY_INFO_PATH, key_ref).await
    }

    async fn attest(&self, challenge: &AttestationChallenge) -> SignResult<AttestationEvidence> {
        self.post(ATTEST_PATH, challenge).await
    }

    async fn sign(&self, request: &RemoteSignRequest) -> SignResult<RemoteSignDispatchResult> {
        self.post_sign(request).await
    }

    async fn query(&self, query: &RemoteOperationQuery) -> SignResult<RemoteOperationStatus> {
        self.post(QUERY_PATH, query).await.map_err(|error| {
            Self::result_unknown(format!(
                "remote signer recovery query could not establish a result: {error}"
            ))
        })
    }
}
