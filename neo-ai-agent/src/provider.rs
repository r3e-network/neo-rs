use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAiCompatible,
    Anthropic,
    Ollama,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialEnv(String);

impl CredentialEnv {
    pub fn new(name: impl Into<String>) -> Result<Self, ProviderError> {
        let name = name.into();
        let valid = !name.is_empty()
            && name.len() <= 128
            && name.chars().all(|character| {
                character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
            })
            && name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_uppercase() || character == '_');
        if !valid {
            return Err(ProviderError::InvalidConfig(
                "credential_env must be an uppercase environment variable name".to_owned(),
            ));
        }
        Ok(Self(name))
    }

    fn resolve(&self) -> Result<String, ProviderError> {
        std::env::var(&self.0).map_err(|_| ProviderError::MissingCredential(self.0.clone()))
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub open_for_ms: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            open_for_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeLimits {
    pub request_timeout_ms: u64,
    pub max_concurrency: usize,
    pub request_budget: u64,
    pub token_budget: u64,
    pub max_retries: u8,
    pub retry_backoff_ms: u64,
    pub circuit_breaker: CircuitBreakerConfig,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            request_timeout_ms: 15_000,
            max_concurrency: 2,
            request_budget: 100,
            token_budget: 100_000,
            max_retries: 2,
            retry_backoff_ms: 100,
            circuit_breaker: CircuitBreakerConfig::default(),
        }
    }
}

impl RuntimeLimits {
    fn validate(self) -> Result<Self, ProviderError> {
        if self.request_timeout_ms == 0
            || self.max_concurrency == 0
            || self.request_budget == 0
            || self.token_budget == 0
            || self.circuit_breaker.failure_threshold == 0
            || self.circuit_breaker.open_for_ms == 0
        {
            return Err(ProviderError::InvalidConfig(
                "runtime limits and circuit-breaker values must be non-zero".to_owned(),
            ));
        }
        if self.max_retries > 5 {
            return Err(ProviderError::InvalidConfig(
                "max_retries must not exceed 5".to_owned(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpProviderConfig {
    pub endpoint: Url,
    pub model: String,
    pub credential_env: Option<CredentialEnv>,
    pub limits: RuntimeLimits,
}

impl HttpProviderConfig {
    pub fn validate(&self, credential_required: bool) -> Result<(), ProviderError> {
        if self.model.trim().is_empty() || self.model.len() > 256 {
            return Err(ProviderError::InvalidConfig(
                "model must be non-empty".to_owned(),
            ));
        }
        if credential_required && self.credential_env.is_none() {
            return Err(ProviderError::InvalidConfig(
                "this provider requires a credential_env reference".to_owned(),
            ));
        }
        if !endpoint_is_allowed(&self.endpoint) {
            return Err(ProviderError::InvalidConfig(
                "endpoint must use HTTPS, except loopback HTTP endpoints".to_owned(),
            ));
        }
        if !self.endpoint.username().is_empty()
            || self.endpoint.password().is_some()
            || self.endpoint.query().is_some()
            || self.endpoint.fragment().is_some()
        {
            return Err(ProviderError::InvalidConfig(
                "endpoint must not embed credentials, query parameters, or fragments".to_owned(),
            ));
        }
        self.limits.validate().map(|_| ())
    }
}

fn endpoint_is_allowed(endpoint: &Url) -> bool {
    if endpoint.scheme() == "https" {
        return true;
    }
    endpoint.scheme() == "http"
        && endpoint.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub system_instruction: String,
    pub evidence_json: String,
    pub max_output_tokens: u32,
}

impl ModelRequest {
    fn estimated_tokens(&self) -> u64 {
        let bytes = self
            .system_instruction
            .len()
            .saturating_add(self.evidence_json.len());
        (bytes as u64).div_ceil(4) + u64::from(self.max_output_tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResponse {
    pub content: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ProviderError>;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    #[error("provider configuration is invalid: {0}")]
    InvalidConfig(String),
    #[error("credential environment variable is unavailable: {0}")]
    MissingCredential(String),
    #[error("provider request timed out")]
    Timeout,
    #[error("provider request or token budget is exhausted")]
    BudgetExhausted,
    #[error("provider circuit breaker is open")]
    CircuitOpen,
    #[error("provider transport failed: {0}")]
    Transport(String),
    #[error("provider returned HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("provider response is invalid: {0}")]
    InvalidResponse(String),
}

impl ProviderError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout | Self::Transport(_))
            || matches!(self, Self::Http { status, .. } if *status == 429 || *status >= 500)
    }
}

#[derive(Debug, Default)]
struct RuntimeState {
    requests_used: u64,
    tokens_reserved: u64,
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

pub struct GovernedProvider<P> {
    inner: P,
    limits: RuntimeLimits,
    concurrency: Arc<Semaphore>,
    state: Mutex<RuntimeState>,
}

impl<P> GovernedProvider<P> {
    pub fn new(inner: P, limits: RuntimeLimits) -> Result<Self, ProviderError> {
        let limits = limits.validate()?;
        Ok(Self {
            inner,
            limits,
            concurrency: Arc::new(Semaphore::new(limits.max_concurrency)),
            state: Mutex::new(RuntimeState::default()),
        })
    }

    fn reserve(&self, request: &ModelRequest) -> Result<(), ProviderError> {
        let mut state = self.state.lock().expect("provider runtime mutex poisoned");
        if state.open_until.is_some_and(|until| until > Instant::now()) {
            return Err(ProviderError::CircuitOpen);
        }
        if state.open_until.is_some() {
            state.open_until = None;
            state.consecutive_failures = 0;
        }
        let tokens = request.estimated_tokens();
        if state.requests_used >= self.limits.request_budget
            || state.tokens_reserved.saturating_add(tokens) > self.limits.token_budget
        {
            return Err(ProviderError::BudgetExhausted);
        }
        state.requests_used += 1;
        state.tokens_reserved += tokens;
        Ok(())
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, ProviderError> {
        self.concurrency
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| ProviderError::Transport("concurrency limiter closed".to_owned()))
    }

    fn record_result(&self, result: &Result<ModelResponse, ProviderError>) {
        let mut state = self.state.lock().expect("provider runtime mutex poisoned");
        match result {
            Ok(_) => state.consecutive_failures = 0,
            Err(error) if error.is_retryable() => {
                state.consecutive_failures = state.consecutive_failures.saturating_add(1);
                if state.consecutive_failures >= self.limits.circuit_breaker.failure_threshold {
                    state.open_until = Some(
                        Instant::now()
                            + Duration::from_millis(self.limits.circuit_breaker.open_for_ms),
                    );
                }
            }
            Err(_) => {}
        }
    }
}

#[async_trait]
impl<P: ModelProvider> ModelProvider for GovernedProvider<P> {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ProviderError> {
        self.reserve(&request)?;
        let _permit = tokio::time::timeout(
            Duration::from_millis(self.limits.request_timeout_ms),
            self.acquire(),
        )
        .await
        .map_err(|_| ProviderError::Timeout)??;
        let mut attempt = 0_u8;
        let result = loop {
            let call = self.inner.complete(request.clone());
            let current = match tokio::time::timeout(
                Duration::from_millis(self.limits.request_timeout_ms),
                call,
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(ProviderError::Timeout),
            };
            if !current.as_ref().is_err_and(ProviderError::is_retryable)
                || attempt >= self.limits.max_retries
            {
                break current;
            }
            attempt += 1;
            let backoff = self
                .limits
                .retry_backoff_ms
                .saturating_mul(u64::from(attempt));
            tokio::time::sleep(Duration::from_millis(backoff)).await;
        };
        self.record_result(&result);
        result
    }
}

struct HttpTransport {
    kind: ProviderKind,
    config: HttpProviderConfig,
    client: reqwest::Client,
}

impl HttpTransport {
    fn new(kind: ProviderKind, config: HttpProviderConfig) -> Result<Self, ProviderError> {
        config.validate(kind != ProviderKind::Ollama)?;
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|error| ProviderError::InvalidConfig(error.to_string()))?;
        Ok(Self {
            kind,
            config,
            client,
        })
    }

    fn request_headers(&self) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(reference) = &self.config.credential_env {
            let credential = reference.resolve()?;
            match self.kind {
                ProviderKind::OpenAiCompatible | ProviderKind::Ollama => {
                    let value =
                        HeaderValue::from_str(&format!("Bearer {credential}")).map_err(|_| {
                            ProviderError::InvalidConfig(
                                "credential contains invalid header bytes".to_owned(),
                            )
                        })?;
                    headers.insert(AUTHORIZATION, value);
                }
                ProviderKind::Anthropic => {
                    let value = HeaderValue::from_str(&credential).map_err(|_| {
                        ProviderError::InvalidConfig(
                            "credential contains invalid header bytes".to_owned(),
                        )
                    })?;
                    headers.insert("x-api-key", value);
                    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
                }
            }
        }
        Ok(headers)
    }

    fn payload(&self, request: &ModelRequest) -> Value {
        match self.kind {
            ProviderKind::OpenAiCompatible => json!({
                "model": self.config.model,
                "temperature": 0,
                "max_tokens": request.max_output_tokens,
                "response_format": {"type": "json_object"},
                "messages": [
                    {"role": "system", "content": request.system_instruction},
                    {"role": "user", "content": request.evidence_json}
                ]
            }),
            ProviderKind::Anthropic => json!({
                "model": self.config.model,
                "temperature": 0,
                "max_tokens": request.max_output_tokens,
                "system": request.system_instruction,
                "messages": [{"role": "user", "content": request.evidence_json}]
            }),
            ProviderKind::Ollama => json!({
                "model": self.config.model,
                "stream": false,
                "format": "json",
                "options": {"temperature": 0, "num_predict": request.max_output_tokens},
                "messages": [
                    {"role": "system", "content": request.system_instruction},
                    {"role": "user", "content": request.evidence_json}
                ]
            }),
        }
    }

    fn parse_response(&self, value: Value) -> Result<ModelResponse, ProviderError> {
        let (content, input_tokens, output_tokens) = match self.kind {
            ProviderKind::OpenAiCompatible => (
                value
                    .pointer("/choices/0/message/content")
                    .and_then(Value::as_str),
                value
                    .pointer("/usage/prompt_tokens")
                    .and_then(Value::as_u64),
                value
                    .pointer("/usage/completion_tokens")
                    .and_then(Value::as_u64),
            ),
            ProviderKind::Anthropic => (
                value.pointer("/content/0/text").and_then(Value::as_str),
                value.pointer("/usage/input_tokens").and_then(Value::as_u64),
                value
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64),
            ),
            ProviderKind::Ollama => (
                value.pointer("/message/content").and_then(Value::as_str),
                value.get("prompt_eval_count").and_then(Value::as_u64),
                value.get("eval_count").and_then(Value::as_u64),
            ),
        };
        let content = content.ok_or_else(|| {
            ProviderError::InvalidResponse("missing structured message content".to_owned())
        })?;
        Ok(ModelResponse {
            content: content.to_owned(),
            input_tokens,
            output_tokens,
        })
    }
}

#[async_trait]
impl ModelProvider for HttpTransport {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ProviderError> {
        let response = self
            .client
            .post(self.config.endpoint.clone())
            .headers(self.request_headers()?)
            .json(&self.payload(&request))
            .send()
            .await
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unreadable response".to_owned());
            return Err(ProviderError::Http {
                status: status.as_u16(),
                message: message.chars().take(512).collect(),
            });
        }
        let value = response
            .json::<Value>()
            .await
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;
        self.parse_response(value)
    }
}

macro_rules! http_provider {
    ($name:ident, $kind:expr) => {
        pub struct $name {
            inner: GovernedProvider<HttpTransport>,
        }

        impl $name {
            pub fn new(config: HttpProviderConfig) -> Result<Self, ProviderError> {
                let limits = config.limits;
                let transport = HttpTransport::new($kind, config)?;
                Ok(Self {
                    inner: GovernedProvider::new(transport, limits)?,
                })
            }
        }

        #[async_trait]
        impl ModelProvider for $name {
            async fn complete(
                &self,
                request: ModelRequest,
            ) -> Result<ModelResponse, ProviderError> {
                self.inner.complete(request).await
            }
        }
    };
}

http_provider!(OpenAiCompatibleProvider, ProviderKind::OpenAiCompatible);
http_provider!(AnthropicProvider, ProviderKind::Anthropic);
http_provider!(OllamaProvider, ProviderKind::Ollama);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    fn spawn_redirect_server(
        location: &str,
    ) -> (Url, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        let address = listener.local_addr().expect("redirect server address");
        let location = location.to_owned();
        let (request_sender, request_receiver) = mpsc::channel();
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set request read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("read provider request");
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            request_sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("capture provider request");
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write redirect response");
        });
        (
            Url::parse(&format!("http://{address}/v1/complete")).expect("provider URL"),
            request_receiver,
            task,
        )
    }

    fn provider_config(endpoint: Url, credential_env: &str) -> HttpProviderConfig {
        let mut limits = RuntimeLimits::default();
        limits.request_timeout_ms = 500;
        limits.max_retries = 0;
        HttpProviderConfig {
            endpoint,
            model: "redirect-security-test".to_owned(),
            credential_env: Some(CredentialEnv::new(credential_env).expect("credential reference")),
            limits,
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            system_instruction: "diagnose only".to_owned(),
            evidence_json: "{}".to_owned(),
            max_output_tokens: 16,
        }
    }

    #[tokio::test]
    async fn all_llm_http_clients_reject_redirects_without_forwarding_credentials() {
        let cases = [
            (
                ProviderKind::OpenAiCompatible,
                "NEO_TEST_OPENAI_REDIRECT_KEY",
            ),
            (ProviderKind::Anthropic, "NEO_TEST_ANTHROPIC_REDIRECT_KEY"),
            (ProviderKind::Ollama, "NEO_TEST_OLLAMA_REDIRECT_KEY"),
        ];

        for (kind, credential_env) in cases {
            let credential = format!("task16-{kind:?}-secret").to_ascii_lowercase();
            // SAFETY: this test owns unique variable names and runs all cases serially.
            unsafe { std::env::set_var(credential_env, &credential) };
            let redirect_target = TcpListener::bind("127.0.0.1:0").expect("bind redirect target");
            redirect_target
                .set_nonblocking(true)
                .expect("set redirect target nonblocking");
            let target_address = redirect_target
                .local_addr()
                .expect("redirect target address");
            let (endpoint, captured_request, redirect_task) =
                spawn_redirect_server(&format!("http://{target_address}/credential-leak"));
            let config = provider_config(endpoint, credential_env);
            let provider: Box<dyn ModelProvider> = match kind {
                ProviderKind::OpenAiCompatible => {
                    Box::new(OpenAiCompatibleProvider::new(config).expect("OpenAI provider"))
                }
                ProviderKind::Anthropic => {
                    Box::new(AnthropicProvider::new(config).expect("Anthropic provider"))
                }
                ProviderKind::Ollama => {
                    Box::new(OllamaProvider::new(config).expect("Ollama provider"))
                }
            };

            let result = provider.complete(request()).await;
            assert!(matches!(
                result,
                Err(ProviderError::Http { status: 302, .. })
            ));
            redirect_task.join().expect("redirect server task");
            let initial_request = captured_request
                .recv_timeout(Duration::from_secs(1))
                .expect("captured initial provider request")
                .to_ascii_lowercase();
            assert!(initial_request.contains(&credential));
            assert!(matches!(
                redirect_target.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ));
            // SAFETY: this test removes only the unique variable it set above.
            unsafe { std::env::remove_var(credential_env) };
        }
    }
}
