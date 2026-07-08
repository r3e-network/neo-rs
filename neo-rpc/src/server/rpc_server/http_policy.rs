use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tower::{BoxError, Layer, Service};

use super::super::jsonrpsee_adapter::{RpcAuthState, verify_basic_auth_header};
use super::super::rpc_server_settings::RpcServerConfig;

#[derive(Clone)]
pub(super) struct RpcBasicAuth {
    user: Arc<str>,
    password: Arc<str>,
}

#[derive(Clone)]
struct RpcCorsConfig {
    allow_origins: Arc<[String]>,
}

impl RpcCorsConfig {
    fn from_settings(settings: &RpcServerConfig) -> Option<Self> {
        if !settings.enable_cors {
            return None;
        }
        let allow_origins = settings
            .allow_origins
            .iter()
            .map(|origin| origin.trim())
            .filter(|origin| !origin.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into();
        Some(Self { allow_origins })
    }

    fn headers_for<Body>(
        &self,
        request: &jsonrpsee::server::HttpRequest<Body>,
    ) -> Option<RpcCorsHeaders> {
        let requested_origin = request
            .headers()
            .get("origin")
            .and_then(|value| value.to_str().ok())?
            .trim();
        if requested_origin.is_empty() {
            return None;
        }

        let allow_any = self
            .allow_origins
            .iter()
            .any(|origin| origin.as_str() == "*");
        let allow_origin = if self.allow_origins.is_empty() || allow_any {
            "*".to_string()
        } else if self
            .allow_origins
            .iter()
            .any(|origin| origin.eq_ignore_ascii_case(requested_origin))
        {
            requested_origin.to_string()
        } else {
            return None;
        };

        let allow_headers = request
            .headers()
            .get("access-control-request-headers")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|headers| !headers.is_empty())
            .unwrap_or("content-type, authorization")
            .to_string();

        Some(RpcCorsHeaders {
            allow_origin,
            allow_headers,
        })
    }
}

#[derive(Clone)]
struct RpcCorsHeaders {
    allow_origin: String,
    allow_headers: String,
}

impl RpcCorsHeaders {
    fn apply(&self, response: &mut jsonrpsee::server::HttpResponse) {
        insert_response_header(
            response,
            "access-control-allow-origin",
            self.allow_origin.as_str(),
        );
        insert_response_header(response, "access-control-allow-methods", "POST, OPTIONS");
        insert_response_header(
            response,
            "access-control-allow-headers",
            self.allow_headers.as_str(),
        );
        insert_response_header(response, "access-control-max-age", "600");
        insert_response_header(response, "vary", "Origin");
    }
}

#[derive(Clone)]
pub(super) struct RpcHttpLayer {
    auth_credentials: Option<Arc<RpcBasicAuth>>,
    cors: Option<Arc<RpcCorsConfig>>,
}

impl RpcHttpLayer {
    pub(super) fn new(
        settings: &RpcServerConfig,
        auth_credentials: Option<Arc<RpcBasicAuth>>,
    ) -> Self {
        Self {
            auth_credentials,
            cors: RpcCorsConfig::from_settings(settings).map(Arc::new),
        }
    }
}

impl<S> Layer<S> for RpcHttpLayer {
    type Service = RpcHttpService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RpcHttpService {
            inner,
            auth_credentials: self.auth_credentials.clone(),
            cors: self.cors.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct RpcHttpService<S> {
    inner: S,
    auth_credentials: Option<Arc<RpcBasicAuth>>,
    cors: Option<Arc<RpcCorsConfig>>,
}

impl<S, Body> Service<jsonrpsee::server::HttpRequest<Body>> for RpcHttpService<S>
where
    S: Service<
            jsonrpsee::server::HttpRequest<Body>,
            Response = jsonrpsee::server::HttpResponse,
            Error = BoxError,
        > + Send
        + 'static,
    S::Future: Send + 'static,
    Body: Send + 'static,
{
    type Response = jsonrpsee::server::HttpResponse;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: jsonrpsee::server::HttpRequest<Body>) -> Self::Future {
        let cors_headers = self
            .cors
            .as_ref()
            .and_then(|cors| cors.headers_for(&request));

        if request.method().as_str().eq_ignore_ascii_case("OPTIONS") && self.cors.is_some() {
            return Box::pin(async move { Ok(preflight_response(cors_headers)) });
        }

        if let Some(credentials) = self.auth_credentials.as_ref() {
            let header = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok());
            if !verify_basic_auth_header(header, &credentials.user, &credentials.password) {
                let mut response = unauthorized_response();
                apply_cors_headers(&mut response, cors_headers.as_ref());
                return Box::pin(async { Ok(response) });
            }
            request
                .extensions_mut()
                .insert(RpcAuthState::authenticated());
        }

        let future = self.inner.call(request);
        Box::pin(async move {
            let mut response = future.await?;
            apply_cors_headers(&mut response, cors_headers.as_ref());
            Ok(response)
        })
    }
}

pub(super) fn auth_credentials_from_settings(
    settings: &RpcServerConfig,
) -> Result<Option<RpcBasicAuth>, &'static str> {
    let user_configured = !settings.rpc_user.trim().is_empty();
    let password_configured = !settings.rpc_pass.trim().is_empty();
    match (user_configured, password_configured) {
        (false, false) => Ok(None),
        (true, true) => Ok(Some(RpcBasicAuth {
            user: Arc::from(settings.rpc_user.as_str()),
            password: Arc::from(settings.rpc_pass.as_str()),
        })),
        _ => Err("rpc_user and rpc_pass must either both be set or both be empty"),
    }
}

fn unauthorized_response() -> jsonrpsee::server::HttpResponse {
    static_response(
        401,
        jsonrpsee::server::HttpBody::from("Authentication required\n"),
        &[
            ("content-type", "text/plain; charset=utf-8"),
            ("www-authenticate", "Basic realm=\"neo-rpc\""),
        ],
    )
}

fn preflight_response(cors_headers: Option<RpcCorsHeaders>) -> jsonrpsee::server::HttpResponse {
    let mut response = if cors_headers.is_some() {
        static_response(204, jsonrpsee::server::HttpBody::empty(), &[])
    } else {
        static_response(
            403,
            jsonrpsee::server::HttpBody::from("CORS origin is not allowed\n"),
            &[("content-type", "text/plain; charset=utf-8")],
        )
    };
    apply_cors_headers(&mut response, cors_headers.as_ref());
    response
}

fn static_response(
    status: u16,
    body: jsonrpsee::server::HttpBody,
    headers: &[(&'static str, &'static str)],
) -> jsonrpsee::server::HttpResponse {
    let mut response = jsonrpsee::server::HttpResponse::new(body);
    if let Ok(status) = status.try_into() {
        *response.status_mut() = status;
    }
    for (name, value) in headers {
        insert_response_header(&mut response, name, value);
    }
    response
}

fn apply_cors_headers(
    response: &mut jsonrpsee::server::HttpResponse,
    cors_headers: Option<&RpcCorsHeaders>,
) {
    if let Some(headers) = cors_headers {
        headers.apply(response);
    }
}

fn insert_response_header(
    response: &mut jsonrpsee::server::HttpResponse,
    name: &'static str,
    value: &str,
) {
    if let Ok(value) = value.parse() {
        response.headers_mut().insert(name, value);
    }
}

#[cfg(test)]
#[path = "../../tests/server/rpc_server/http_policy.rs"]
mod tests;
