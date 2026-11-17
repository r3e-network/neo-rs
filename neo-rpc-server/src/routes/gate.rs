//! Checks applied at the HTTP boundary, before a request body is read.
//!
//! The dispatcher in `handlers` authenticates each JSON-RPC call, but only
//! after the body has been aggregated and parsed. The gate runs first, on the
//! request head alone, so a client that cannot authenticate never makes the
//! server read or parse anything:
//!
//! 1. per-peer-IP admission limit,
//! 2. HTTP Basic authentication, when credentials are configured,
//! 3. the routes (body read, parse, method-tier limit, handler).
//!
//! The dispatcher keeps its own authentication check as defence in depth.

use super::cors::{BasicAuth, CorsConfig, apply_cors, verify_basic_auth};
use super::{build_http_response, error_response};
use crate::client_identity;
use crate::middleware::{GovernorRateLimiter, RateLimitConfig};
use crate::rpc_error::RpcError;
use crate::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};
use crate::rpc_server_settings::RpcServerConfig;
use hyper::header::{AUTHORIZATION, HeaderValue, ORIGIN};
use hyper::service::Service;
use hyper::{Body, Method, Request, Response, StatusCode};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

pub(crate) struct HttpGate {
    auth: Arc<Option<BasicAuth>>,
    /// Budget of its own, never the per-method limiter: an admitted call is
    /// still charged exactly once, at its method tier.
    admission: Option<GovernorRateLimiter>,
    cors: Option<CorsConfig>,
}

impl HttpGate {
    pub(crate) fn new(auth: Arc<Option<BasicAuth>>, settings: &RpcServerConfig) -> Self {
        let admission = settings
            .admission_rate_limit()
            .map(|(max_rps, burst)| GovernorRateLimiter::new(RateLimitConfig { max_rps, burst }));
        Self {
            auth,
            admission,
            cors: CorsConfig::from_settings_quiet(settings),
        }
    }

    /// Serves one request from `remote` with that peer bound as the client
    /// identity, so admission, method-tier limiting and session quotas are
    /// all attributed to the same address: the TCP peer, never a
    /// client-supplied forwarding header.
    pub(crate) async fn serve_scoped<S>(
        self: Arc<Self>,
        remote: SocketAddr,
        request: Request<Body>,
        service: S,
    ) -> Result<Response<Body>, S::Error>
    where
        S: Service<Request<Body>, Response = Response<Body>>,
    {
        client_identity::scope_client_key(
            remote.ip().to_string(),
            self.serve(remote, request, service),
        )
        .await
    }

    async fn serve<S>(
        &self,
        remote: SocketAddr,
        mut request: Request<Body>,
        mut service: S,
    ) -> Result<Response<Body>, S::Error>
    where
        S: Service<Request<Body>, Response = Response<Body>>,
    {
        if let Some(refusal) = self.refuse(remote.ip(), &request) {
            // The body is dropped unread; hyper then closes the connection
            // rather than draining what the client still wants to send.
            return Ok(refusal);
        }
        request.extensions_mut().insert(remote);
        service.call(request).await
    }

    /// Decides from the request head alone whether to refuse the request.
    fn refuse<B>(&self, peer: IpAddr, request: &Request<B>) -> Option<Response<Body>> {
        // A CORS preflight carries neither credentials nor a body; its route
        // answers it without doing any work on behalf of the client.
        if request.method() == Method::OPTIONS {
            return None;
        }
        let origin = request.headers().get(ORIGIN);

        if let Some(limiter) = &self.admission
            && limiter.check(peer).is_blocked()
        {
            return Some(self.refusal(
                RpcError::too_many_requests(),
                StatusCode::TOO_MANY_REQUESTS,
                origin,
            ));
        }

        let auth = self.auth.as_ref().as_ref()?;
        let header = request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        if verify_basic_auth(header, auth) {
            return None;
        }
        Some(self.refusal(RpcError::access_denied(), StatusCode::UNAUTHORIZED, origin))
    }

    /// A transport-level refusal. The JSON-RPC id is unknown because the body
    /// has not been read, so the error carries a null id.
    fn refusal(
        &self,
        error: RpcError,
        status: StatusCode,
        origin: Option<&HeaderValue>,
    ) -> Response<Body> {
        RPC_REQ_TOTAL.inc();
        RPC_ERR_TOTAL.inc();
        let unauthorized = status == StatusCode::UNAUTHORIZED;
        let mut response = build_http_response(
            Some(error_response(None, error)),
            unauthorized,
            unauthorized,
        );
        *response.status_mut() = status;
        apply_cors(&mut response, self.cors.as_ref(), origin);
        response
    }
}
